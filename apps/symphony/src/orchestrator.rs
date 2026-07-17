use chrono::{DateTime, Utc};
use regex::Regex;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::Duration;

use crate::codex::app_server;
use crate::config;
use crate::domain::{
    AgentBackend, AgentEvent, CodexConfig, CodexTotals, CompletedEntry, ContextEntry, ContextScope,
    EscalationRequest, EscalationResponse, EventKind, EventSeverity, HooksConfig, Issue,
    OrchestratorSnapshot, OrchestratorState, PendingEscalation, PiAgentConfig, PollingSnapshot,
    RateLimitInfo, RefreshRequestOutcome, RetryEntry, RetrySnapshotEntry, RunAttempt,
    RunningSessionSnapshot, ServiceConfig, SessionTokenUsage, SupervisorSnapshot, TrackerConfig,
    WorkerSessionInfo, WorkspaceConfig, WorkspaceIsolation,
};
use crate::error::{Result, SymphonyError};
use crate::event_stream::EventHub;
use crate::github::auth::{github_token_source_name, resolve_github_token};
use crate::linear::adapter::TrackerAdapter;
use crate::notifications;
use crate::pi_agent::rpc_bridge;
use crate::session_summary::{compact_session_id, normalize_whitespace, truncate_for_display};
use crate::shared_context::SharedContextStore;
use crate::ssh::{self, WorkerHostSelection};
use crate::supervisor::{SupervisorAgent, SupervisorDependencies};
use crate::triage::runtime::TriageRuntime;
use crate::workflow_store::WorkflowStore;
use crate::{docker, path_safety, prompt_builder, workspace};

static SHARED_CONTEXT_PLACEHOLDER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\{\{\s*shared_context\s*\}\}")
        .expect("shared_context placeholder regex must compile")
});
static RATE_LIMIT_WINDOW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\d+)\s*(hours?|hrs?|hr|h|minutes?|mins?|min|m|seconds?|secs?|sec|s)")
        .expect("rate-limit window regex must compile")
});
const WORKER_LAST_ERROR_MAX_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandOutcome {
    success: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AgentReviewPrStatus {
    Valid {
        branch: String,
        pr_url: String,
    },
    Missing {
        branch: Option<String>,
        reason: String,
    },
    /// Check could not complete (timeout, binary missing, etc.).
    /// Callers should NOT demote the issue on transient failures.
    CheckFailed {
        reason: String,
    },
}

#[derive(Debug, Deserialize)]
struct PullRequestViewSummary {
    url: String,
    state: String,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(rename = "baseRefName")]
    base_ref_name: String,
}

const SUBPROCESS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn run_command_outcome(
    program: &str,
    args: &[&str],
    cwd: &Path,
) -> std::result::Result<CommandOutcome, String> {
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GH_PROMPT_DISABLED", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("{} {} failed to start: {}", program, args.join(" "), err))?;

    let timeout = SUBPROCESS_TIMEOUT;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child.stdout.take().map_or_else(String::new, |mut r| {
                    let mut s = String::new();
                    let _ = std::io::Read::read_to_string(&mut r, &mut s);
                    s
                });
                let stderr = child.stderr.take().map_or_else(String::new, |mut r| {
                    let mut s = String::new();
                    let _ = std::io::Read::read_to_string(&mut r, &mut s);
                    s
                });
                return Ok(CommandOutcome {
                    success: status.success(),
                    stdout: stdout.trim().to_string(),
                    stderr: stderr.trim().to_string(),
                });
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "{} {} timed out after {}s",
                        program,
                        args.join(" "),
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(err) => {
                return Err(format!(
                    "{} {} failed while waiting: {}",
                    program,
                    args.join(" "),
                    err
                ));
            }
        }
    }
}

fn check_agent_review_pr_status_with<F>(
    workspace_path: &Path,
    expected_base_branch: Option<&str>,
    mut run: F,
) -> AgentReviewPrStatus
where
    F: FnMut(&str, &[&str], &Path) -> std::result::Result<CommandOutcome, String>,
{
    let branch_result = match run("git", &["branch", "--show-current"], workspace_path) {
        Ok(result) => result,
        Err(err) => {
            return AgentReviewPrStatus::CheckFailed {
                reason: format!("could not determine current branch: {err}"),
            };
        }
    };

    if !branch_result.success {
        let detail = if branch_result.stderr.is_empty() {
            branch_result.stdout
        } else {
            branch_result.stderr
        };
        return AgentReviewPrStatus::Missing {
            branch: None,
            reason: format!("could not determine current branch: {detail}"),
        };
    }

    let branch = branch_result.stdout.trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        return AgentReviewPrStatus::Missing {
            branch: None,
            reason: "current branch is detached or empty".to_string(),
        };
    }

    let remote_result = match run(
        "git",
        &[
            "ls-remote",
            "--exit-code",
            "--heads",
            "origin",
            branch.as_str(),
        ],
        workspace_path,
    ) {
        Ok(result) => result,
        Err(err) => {
            return AgentReviewPrStatus::CheckFailed {
                reason: format!("could not verify remote branch: {err}"),
            };
        }
    };

    if !remote_result.success {
        let detail = if !remote_result.stderr.is_empty() {
            remote_result.stderr
        } else if !remote_result.stdout.is_empty() {
            remote_result.stdout
        } else {
            "ref not found".to_string()
        };
        return AgentReviewPrStatus::Missing {
            branch: Some(branch),
            reason: format!("remote branch is missing on origin ({detail})"),
        };
    }

    let pr_result = match run(
        "gh",
        &["pr", "view", "--json", "url,state,headRefName,baseRefName"],
        workspace_path,
    ) {
        Ok(result) => result,
        Err(err) => {
            return AgentReviewPrStatus::CheckFailed {
                reason: format!("could not verify open PR: {err}"),
            };
        }
    };

    if !pr_result.success {
        let detail = if pr_result.stderr.is_empty() {
            pr_result.stdout
        } else {
            pr_result.stderr
        };
        let reason = if detail
            .to_ascii_lowercase()
            .contains("no pull requests found for branch")
        {
            format!("no open PR found for current branch `{branch}`")
        } else {
            format!("could not verify open PR: {detail}")
        };
        return AgentReviewPrStatus::Missing {
            branch: Some(branch),
            reason,
        };
    }

    let pr: PullRequestViewSummary = match serde_json::from_str(&pr_result.stdout) {
        Ok(pr) => pr,
        Err(err) => {
            return AgentReviewPrStatus::Missing {
                branch: Some(branch),
                reason: format!("could not parse `gh pr view` output: {err}"),
            };
        }
    };

    if !pr.state.eq_ignore_ascii_case("OPEN") {
        return AgentReviewPrStatus::Missing {
            branch: Some(branch),
            reason: format!("PR exists but is not open (state: {})", pr.state),
        };
    }

    if pr.head_ref_name != branch {
        return AgentReviewPrStatus::Missing {
            branch: Some(branch.clone()),
            reason: format!(
                "PR head branch `{}` does not match current branch `{}`",
                pr.head_ref_name, branch
            ),
        };
    }

    if let Some(base_branch) = expected_base_branch
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if pr.base_ref_name != base_branch {
            return AgentReviewPrStatus::Missing {
                branch: Some(branch),
                reason: format!(
                    "PR base branch `{}` does not match expected `{}`",
                    pr.base_ref_name, base_branch
                ),
            };
        }
    }

    AgentReviewPrStatus::Valid {
        branch,
        pr_url: pr.url,
    }
}

fn check_agent_review_pr_status(
    workspace_path: &Path,
    expected_base_branch: Option<&str>,
) -> AgentReviewPrStatus {
    check_agent_review_pr_status_with(workspace_path, expected_base_branch, run_command_outcome)
}

fn invalid_agent_review_note(issue: &Issue, branch: Option<&str>, reason: &str) -> String {
    let branch_display = branch.unwrap_or("unknown");
    format!(
        "## Symphony note\n\nMoved this issue from `Agent Review` back to `In Progress` because Agent Review requires an open PR for the current workspace branch.\n\n- branch: `{branch_display}`\n- reason: {reason}\n\nOpen or restore the PR, then move `{}` back to `Agent Review`.",
        issue.identifier
    )
}

#[derive(Debug, Clone)]
pub struct CompletionCommentBuilder<'a> {
    issue_identifier: &'a str,
    terminal_state: &'a str,
    turn_count: u32,
    total_tokens: u64,
    duration: chrono::Duration,
    worker_host: Option<&'a str>,
}

impl<'a> CompletionCommentBuilder<'a> {
    pub fn new(
        issue_identifier: &'a str,
        terminal_state: &'a str,
        turn_count: u32,
        total_tokens: u64,
        duration: chrono::Duration,
        worker_host: Option<&'a str>,
    ) -> Self {
        Self {
            issue_identifier,
            terminal_state,
            turn_count,
            total_tokens,
            duration,
            worker_host,
        }
    }

    pub fn build(&self) -> String {
        format!(
            "## Symphony Execution Summary\n\n**Issue:** {}\n**Status:** {}\n**Turns:** {}\n**Tokens:** {}\n**Duration:** {}\n**Worker:** {}",
            self.issue_identifier,
            self.terminal_state,
            self.turn_count,
            self.total_tokens,
            format_elapsed_duration(self.duration),
            self.worker_host.unwrap_or("local")
        )
    }
}

fn format_elapsed_duration(duration: chrono::Duration) -> String {
    let total_seconds = duration.num_seconds().max(0);
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

pub fn enrich_escalation_payload(
    payload: &mut serde_json::Value,
    issue_identifier: &str,
    issue_state: Option<&str>,
    parent_identifier: Option<&str>,
) {
    let mut payload_object = match std::mem::take(payload) {
        serde_json::Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("raw_payload".to_string(), other);
            map
        }
    };

    payload_object.insert(
        "issue_identifier".to_string(),
        serde_json::Value::String(issue_identifier.to_string()),
    );
    payload_object.insert(
        "issue_state".to_string(),
        issue_state
            .map(|state| serde_json::Value::String(state.to_string()))
            .unwrap_or(serde_json::Value::Null),
    );
    payload_object.insert(
        "parent_identifier".to_string(),
        parent_identifier
            .map(|identifier| serde_json::Value::String(identifier.to_string()))
            .unwrap_or(serde_json::Value::Null),
    );

    *payload = serde_json::Value::Object(payload_object);
}

fn completion_comments_enabled(tracker: &TrackerConfig) -> bool {
    tracker
        .kind
        .as_deref()
        .is_some_and(|kind| kind.eq_ignore_ascii_case("github"))
}

fn workflow_dir_from_path(workflow_path: &Path) -> PathBuf {
    workflow_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

// ── Standalone Worker Task ──────────────────────────────────────────────

/// All configuration needed by a spawned worker task.
/// Bundled into a struct to avoid too-many-arguments lint.
struct WorkerTaskConfig {
    workspace: WorkspaceConfig,
    hooks: HooksConfig,
    codex: CodexConfig,
    pi_agent: PiAgentConfig,
    pi_model_override: Option<String>,
    agent_backend: AgentBackend,
    max_turns: u32,
    tracker: TrackerConfig,
    prompt_template: String,
    shared_context: String,
    workspace_refresh_policy: workspace::ExistingWorkspaceRefreshPolicy,
    event_tx: tokio::sync::mpsc::UnboundedSender<(String, AgentEvent)>,
    escalation_tx: tokio::sync::mpsc::UnboundedSender<rpc_bridge::EscalationDispatch>,
    escalation_timeout_ms: u64,
    /// Canonical WORKFLOW.md path passed to worker helper scripts.
    workflow_path: PathBuf,
}

enum IssueCheck {
    Continue(Issue),
    Done(Issue),
    Error(SymphonyError),
}

struct SessionTurnLoopSuccess {
    events: Vec<AgentEvent>,
    metrics: Option<TurnMetrics>,
    schedule_continuation: bool,
}

struct SessionTurnLoopFailure {
    error: SymphonyError,
    events: Vec<AgentEvent>,
    metrics: Option<TurnMetrics>,
}

fn accumulate_turn_metrics(
    metrics: &mut Option<TurnMetrics>,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    rate_limits: Option<serde_json::Value>,
) {
    match metrics {
        Some(total) => {
            total.input_tokens = total.input_tokens.saturating_add(input_tokens);
            total.output_tokens = total.output_tokens.saturating_add(output_tokens);
            total.total_tokens = total.total_tokens.saturating_add(total_tokens);
            if let Some(rate_limits) = rate_limits {
                total.rate_limits = Some(rate_limits);
            }
        }
        None => {
            *metrics = Some(TurnMetrics {
                input_tokens,
                output_tokens,
                total_tokens,
                rate_limits,
            });
        }
    }
}

fn is_terminal_state(state_name: &str, tracker_config: &TrackerConfig) -> bool {
    let normalized = normalize_issue_state(state_name);
    tracker_config
        .terminal_states
        .iter()
        .any(|state| normalize_issue_state(state) == normalized)
}

fn is_active_state(state_name: &str, tracker_config: &TrackerConfig) -> bool {
    let normalized = normalize_issue_state(state_name);
    tracker_config
        .active_states
        .iter()
        .any(|state| normalize_issue_state(state) == normalized)
}

fn backend_stall_timeout_ms(config: &ServiceConfig, backend: AgentBackend) -> i64 {
    let timeout = match backend {
        AgentBackend::KataCli => config.pi_agent.stall_timeout_ms,
        AgentBackend::Codex => config.codex.stall_timeout_ms,
    };
    timeout.min(i64::MAX as u64) as i64
}

fn effective_pi_model_for_issue(config: &ServiceConfig, issue: &Issue) -> Option<String> {
    for label in &issue.labels {
        let normalized_label = label.trim().to_lowercase();
        if normalized_label.is_empty() {
            continue;
        }

        if let Some(model) = config.pi_agent.model_by_label.get(&normalized_label) {
            tracing::info!(
                event = "model_resolved_from_label",
                issue_identifier = %issue.identifier,
                label = %normalized_label,
                model = %model,
                "resolved pi-agent model from issue label"
            );
            return Some(model.clone());
        }
    }

    config.pi_agent.model_for_state(&issue.state)
}

/// Determine whether a multi-turn session should continue after a turn completes.
///
/// Returns `true` only if:
/// - The issue is still assigned to this worker
/// - The issue is in an active (non-terminal) state
/// - The issue state has NOT changed from the state it was dispatched with
///
/// A state change (e.g. In Progress → Agent Review) means the orchestrator
/// should end this session and dispatch a new one with the appropriate per-state
/// prompt. Without this check, the multi-turn loop continues with a stale prompt
/// and the agent never receives the instructions for the new state.
fn should_continue_issue_in_session(
    issue: &Issue,
    tracker_config: &TrackerConfig,
    dispatched_state: &str,
) -> bool {
    issue.assigned_to_worker
        && is_active_state(&issue.state, tracker_config)
        && !is_terminal_state(&issue.state, tracker_config)
        && normalize_issue_state(&issue.state) == normalize_issue_state(dispatched_state)
}

/// Build a boxed `TrackerAdapter` appropriate for the given `TrackerConfig`.
/// Used for inter-turn issue state refresh — routes to GitHub or Linear based on `tracker.kind`.
async fn build_tracker_adapter(tracker_config: &TrackerConfig) -> Box<dyn TrackerAdapter> {
    let kind = tracker_config.kind.as_deref().unwrap_or("linear");
    if kind.eq_ignore_ascii_case("github") {
        use crate::github::adapter::GithubAdapter;
        use crate::github::client::GithubClient;
        let resolved_token = match tokio::task::spawn_blocking({
            let tracker_config = tracker_config.clone();
            move || resolve_github_token(&tracker_config)
        })
        .await
        {
            Ok(resolved) => resolved,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "failed to join blocking GitHub token resolution task for inter-turn refresh"
                );
                None
            }
        };
        let token = resolved_token
            .map(|resolved| {
                tracing::debug!(
                    token_source = github_token_source_name(resolved.source),
                    "resolved GitHub token source for inter-turn refresh"
                );
                resolved.token
            })
            .unwrap_or_else(|| {
                tracing::warn!(
                    "no GitHub token found for inter-turn refresh; requests will likely fail"
                );
                String::new()
            });
        let repo_owner = tracker_config.repo_owner.clone().unwrap_or_default();
        let repo_name = tracker_config.repo_name.clone().unwrap_or_default();
        let label_prefix = tracker_config
            .label_prefix
            .clone()
            .unwrap_or_else(|| "symphony".to_string());
        let endpoint = tracker_config.endpoint.trim();
        let endpoint = if endpoint.is_empty() {
            "https://api.github.com"
        } else {
            endpoint
        };
        let client =
            GithubClient::with_base_url(token, repo_owner, repo_name, label_prefix, endpoint);
        Box::new(GithubAdapter::new(client, tracker_config.clone()))
    } else {
        use crate::linear::adapter::LinearAdapter;
        use crate::linear::client::LinearClient;
        Box::new(LinearAdapter::new(LinearClient::new(
            tracker_config.clone(),
        )))
    }
}

async fn check_issue_still_active(
    issue: &Issue,
    adapter: &dyn TrackerAdapter,
    tracker_config: &TrackerConfig,
    dispatched_state: &str,
) -> IssueCheck {
    match adapter
        .fetch_issue_states_by_ids(std::slice::from_ref(&issue.id))
        .await
    {
        Ok(issues) => match issues.first() {
            Some(refreshed) => {
                if should_continue_issue_in_session(refreshed, tracker_config, dispatched_state) {
                    IssueCheck::Continue(refreshed.clone())
                } else {
                    if is_active_state(&refreshed.state, tracker_config)
                        && normalize_issue_state(&refreshed.state)
                            != normalize_issue_state(dispatched_state)
                    {
                        tracing::info!(
                            issue_id = %refreshed.id,
                            issue_identifier = %refreshed.identifier,
                            dispatched_state = %dispatched_state,
                            current_state = %refreshed.state,
                            "issue state changed during session; ending session for re-dispatch with new prompt"
                        );
                    }
                    IssueCheck::Done(refreshed.clone())
                }
            }
            None => IssueCheck::Done(issue.clone()),
        },
        Err(err) => IssueCheck::Error(err),
    }
}

async fn run_codex_turns_in_session<E, EFut, EventCallback>(
    session: &mut app_server::SessionHandle,
    issue: &Issue,
    initial_prompt: String,
    max_turns: u32,
    tracker_config: &TrackerConfig,
    graphql_executor: E,
    mut stream_event: EventCallback,
) -> std::result::Result<SessionTurnLoopSuccess, SessionTurnLoopFailure>
where
    E: Fn(String, serde_json::Value) -> EFut + Clone + Send,
    EFut: Future<Output = Result<serde_json::Value>> + Send,
    EventCallback: FnMut(AgentEvent) + Send,
{
    let capped_max_turns = max_turns.max(1);
    let mut turn_number: u32 = 1;
    let mut current_issue = issue.clone();
    let issue_state_client = build_tracker_adapter(tracker_config).await;
    let mut observed_events: Vec<AgentEvent> = Vec::new();
    let mut metrics: Option<TurnMetrics> = None;
    let mut schedule_continuation = true;
    let mut initial_prompt = Some(initial_prompt);

    loop {
        let prompt = if turn_number == 1 {
            initial_prompt.take().unwrap_or_default()
        } else {
            prompt_builder::render_continuation_prompt(turn_number, capped_max_turns)
        };

        let run_result =
            app_server::run_turn(session, &prompt, graphql_executor.clone(), |event| {
                stream_event(event.clone());
                observed_events.push(event);
            })
            .await;

        match run_result {
            Ok(turn_result) => {
                accumulate_turn_metrics(
                    &mut metrics,
                    turn_result.input_tokens,
                    turn_result.output_tokens,
                    turn_result.total_tokens,
                    turn_result.rate_limits.clone(),
                );
            }
            Err(err) => {
                return Err(SessionTurnLoopFailure {
                    error: err,
                    events: observed_events,
                    metrics,
                });
            }
        }

        if turn_number >= capped_max_turns {
            break;
        }

        match check_issue_still_active(
            &current_issue,
            issue_state_client.as_ref(),
            tracker_config,
            &issue.state,
        )
        .await
        {
            IssueCheck::Continue(refreshed) => {
                current_issue = refreshed;
                turn_number = turn_number.saturating_add(1);
            }
            IssueCheck::Done(_refreshed) => {
                schedule_continuation = false;
                break;
            }
            IssueCheck::Error(err) => {
                let event = AgentEvent::Notification {
                    timestamp: Utc::now(),
                    codex_app_server_pid: None,
                    message: format!(
                        "inter-turn issue refresh failed for {} ({}): {}",
                        current_issue.identifier, current_issue.id, err
                    ),
                };
                stream_event(event.clone());
                observed_events.push(event);
                tracing::warn!(
                    issue_id = %current_issue.id,
                    issue_identifier = %current_issue.identifier,
                    error = %err,
                    "failed to refresh issue state between worker turns; ending session-level turn loop"
                );
                break;
            }
        }
    }

    Ok(SessionTurnLoopSuccess {
        events: observed_events,
        metrics,
        schedule_continuation,
    })
}

async fn run_pi_turns_in_session<EventCallback>(
    session: &mut rpc_bridge::SessionHandle,
    issue: &Issue,
    initial_prompt: String,
    max_turns: u32,
    tracker_config: &TrackerConfig,
    steer_rx: &mut Option<tokio::sync::mpsc::UnboundedReceiver<rpc_bridge::FollowUpRequest>>,
    mut stream_event: EventCallback,
) -> std::result::Result<SessionTurnLoopSuccess, SessionTurnLoopFailure>
where
    EventCallback: FnMut(AgentEvent) + Send,
{
    let capped_max_turns = max_turns.max(1);
    let mut turn_number: u32 = 1;
    let mut current_issue = issue.clone();
    let issue_state_client = build_tracker_adapter(tracker_config).await;
    let mut observed_events: Vec<AgentEvent> = Vec::new();
    let mut metrics: Option<TurnMetrics> = None;
    let mut schedule_continuation = true;
    let mut initial_prompt = Some(initial_prompt);

    loop {
        let prompt = if turn_number == 1 {
            initial_prompt.take().unwrap_or_default()
        } else {
            prompt_builder::render_continuation_prompt(turn_number, capped_max_turns)
        };

        let run_result =
            rpc_bridge::run_turn_with_followups(session, &prompt, steer_rx.as_mut(), |event| {
                stream_event(event.clone());
                observed_events.push(event);
            })
            .await;

        match run_result {
            Ok(turn_result) => {
                accumulate_turn_metrics(
                    &mut metrics,
                    turn_result.input_tokens,
                    turn_result.output_tokens,
                    turn_result.total_tokens,
                    turn_result.rate_limits.clone(),
                );
            }
            Err(err) => {
                return Err(SessionTurnLoopFailure {
                    error: err,
                    events: observed_events,
                    metrics,
                });
            }
        }

        if turn_number >= capped_max_turns {
            break;
        }

        match check_issue_still_active(
            &current_issue,
            issue_state_client.as_ref(),
            tracker_config,
            &issue.state,
        )
        .await
        {
            IssueCheck::Continue(refreshed) => {
                current_issue = refreshed;
                turn_number = turn_number.saturating_add(1);
            }
            IssueCheck::Done(_refreshed) => {
                schedule_continuation = false;
                break;
            }
            IssueCheck::Error(err) => {
                let event = AgentEvent::Notification {
                    timestamp: Utc::now(),
                    codex_app_server_pid: None,
                    message: format!(
                        "inter-turn issue refresh failed for {} ({}): {}",
                        current_issue.identifier, current_issue.id, err
                    ),
                };
                stream_event(event.clone());
                observed_events.push(event);
                tracing::warn!(
                    issue_id = %current_issue.id,
                    issue_identifier = %current_issue.identifier,
                    error = %err,
                    "failed to refresh issue state between worker turns; ending session-level turn loop"
                );
                break;
            }
        }
    }

    Ok(SessionTurnLoopSuccess {
        events: observed_events,
        metrics,
        schedule_continuation,
    })
}

/// Run the full worker lifecycle for a single issue. This function is
/// designed to run in a spawned tokio task — it takes owned/cloned data
/// and does not require `&mut Orchestrator`.
///
/// Steps: ensure workspace → before_run hook → render prompt → start
/// Codex session → run up to max_turns on one session → stop session → after_run hook.
async fn run_worker_task(
    issue: &Issue,
    attempt: Option<u32>,
    worker_host: Option<&str>,
    config: &WorkerTaskConfig,
    mut steer_rx: Option<tokio::sync::mpsc::UnboundedReceiver<rpc_bridge::FollowUpRequest>>,
) -> WorkerResult {
    let issue_id = issue.id.clone();

    if config.workspace.isolation == WorkspaceIsolation::Docker {
        let docker_config = config.workspace.docker.clone().unwrap_or_default();

        if !docker::is_docker_available().await {
            return WorkerResult {
                issue_id,
                completion: WorkerCompletion::Failed {
                    error: SymphonyError::DockerNotAvailable.to_string(),
                },
                events: vec![],
                metrics: None,
            };
        }

        let image =
            match docker::resolve_image(&docker_config.image, docker_config.setup.as_deref()).await
            {
                Ok(image) => image,
                Err(err) => {
                    return WorkerResult {
                        issue_id,
                        completion: WorkerCompletion::Failed {
                            error: format!("docker image resolution failed: {err}"),
                        },
                        events: vec![],
                        metrics: None,
                    };
                }
            };

        let env_values: Vec<(&str, String)> = ["LINEAR_API_KEY", "GH_TOKEN", "GITHUB_TOKEN"]
            .into_iter()
            .filter_map(|key| {
                std::env::var(key)
                    .ok()
                    .filter(|value| !value.is_empty())
                    .map(|value| (key, value))
            })
            .collect();
        let env_refs: Vec<(&str, &str)> = env_values
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();

        let container_id =
            match docker::start_container(&image, issue, &docker_config, &env_refs).await {
                Ok(id) => id,
                Err(err) => {
                    return WorkerResult {
                        issue_id,
                        completion: WorkerCompletion::Failed {
                            error: format!("docker container start failed: {err}"),
                        },
                        events: vec![],
                        metrics: None,
                    };
                }
            };

        let docker_result: std::result::Result<(WorkerCompletion, Option<TurnMetrics>), String> =
            async {
                workspace::docker_bootstrap_repository(
                    &container_id,
                    &config.workspace,
                    &issue.identifier,
                )
                .await
                .map_err(|err| format!("docker workspace bootstrap failed: {err}"))?;

                // TODO: inject skills into Docker container via `docker cp`
                // For now, skills injection is only supported for local isolation.

                let hook_cwd = workflow_dir_from_path(&config.workflow_path);

                if let Some(hook) = &config.hooks.after_create {
                    workspace::run_hook_in_container(
                        "after_create",
                        &container_id,
                        hook,
                        issue,
                        config.hooks.timeout_ms,
                        &hook_cwd,
                    )
                    .await
                    .map_err(|err| format!("after_create hook failed: {err}"))?;
                }

                if let Some(hook) = &config.hooks.before_run {
                    workspace::run_hook_in_container(
                        "before_run",
                        &container_id,
                        hook,
                        issue,
                        config.hooks.timeout_ms,
                        &hook_cwd,
                    )
                    .await
                    .map_err(|err| format!("before_run hook failed: {err}"))?;
                }

                let prompt = prompt_builder::render_prompt_with_shared_context(
                    &config.prompt_template,
                    issue,
                    attempt,
                    config.workspace.base_branch.as_deref(),
                    &config.shared_context,
                )
                .map_err(|err| format!("prompt rendering failed: {err}"))?;

                let loop_result = match config.agent_backend {
                    AgentBackend::Codex => {
                        let symphony_bin = std::env::current_exe()
                            .ok()
                            .map(|path| path.to_string_lossy().to_string());
                        let symphony_workflow_path =
                            config.workflow_path.to_string_lossy().to_string();
                        let mut session = app_server::start_session_with_helper_env(
                            &config.codex,
                            issue,
                            Path::new("/workspace"),
                            Path::new("/"),
                            None,
                            Some(&container_id),
                            app_server::HelperEnv {
                                symphony_bin: symphony_bin.as_deref(),
                                symphony_workflow_path: Some(symphony_workflow_path.as_str()),
                            },
                        )
                        .await
                        .map_err(|err| format!("codex session start failed: {err}"))?;

                        tracing::info!(
                            event = "worker_started",
                            backend = "codex",
                            issue_id = %issue.id,
                            issue_identifier = %issue.identifier,
                            session_id = %session.session_id,
                            workspace_path = "/workspace",
                            container_id = %container_id,
                            "docker worker attempt started"
                        );

                        let linear_client =
                            crate::linear::client::LinearClient::new(config.tracker.clone());
                        let graphql_executor = move |query: String, vars: serde_json::Value| {
                            let client = linear_client.clone();
                            async move { client.graphql_raw(&query, vars).await }
                        };

                        let loop_result = run_codex_turns_in_session(
                            &mut session,
                            issue,
                            prompt.clone(),
                            config.max_turns,
                            &config.tracker,
                            graphql_executor,
                            {
                                let event_tx = config.event_tx.clone();
                                let issue_id = issue.id.clone();
                                move |event| {
                                    let _ = event_tx.send((issue_id.clone(), event));
                                }
                            },
                        )
                        .await;

                        if let Err(err) = app_server::stop_session(session).await {
                            tracing::warn!(
                                issue_id = %issue.id,
                                issue_identifier = %issue.identifier,
                                error = %err,
                                "failed to stop codex session cleanly"
                            );
                        }

                        loop_result
                    }
                    AgentBackend::KataCli => {
                        let mut session = rpc_bridge::start_session(
                            &config.pi_agent,
                            issue,
                            Path::new("/workspace"),
                            Path::new("/"),
                            rpc_bridge::StartSessionOptions {
                                worker_host: None,
                                container_id: Some(container_id.clone()),
                                escalation_tx: config.escalation_tx.clone(),
                                escalation_timeout_ms: config.escalation_timeout_ms,
                                model_override: config.pi_model_override.clone(),
                                symphony_bin: std::env::current_exe()
                                    .ok()
                                    .map(|path| path.to_string_lossy().to_string()),
                                symphony_workflow_path: Some(
                                    config.workflow_path.to_string_lossy().to_string(),
                                ),
                            },
                        )
                        .await
                        .map_err(|err| format!("pi session start failed: {err}"))?;

                        tracing::info!(
                            event = "worker_started",
                            backend = "pi",
                            issue_id = %issue.id,
                            issue_identifier = %issue.identifier,
                            session_id = %session.session_id,
                            workspace_path = "/workspace",
                            container_id = %container_id,
                            "docker worker attempt started"
                        );

                        let loop_result = run_pi_turns_in_session(
                            &mut session,
                            issue,
                            prompt,
                            config.max_turns,
                            &config.tracker,
                            &mut steer_rx,
                            {
                                let event_tx = config.event_tx.clone();
                                let issue_id = issue.id.clone();
                                move |event| {
                                    let _ = event_tx.send((issue_id.clone(), event));
                                }
                            },
                        )
                        .await;

                        if let Err(err) = rpc_bridge::stop_session(session).await {
                            tracing::warn!(
                                issue_id = %issue.id,
                                issue_identifier = %issue.identifier,
                                error = %err,
                                "failed to stop pi session cleanly"
                            );
                        }

                        loop_result
                    }
                };

                if let Some(hook) = &config.hooks.after_run {
                    if let Err(err) = workspace::run_hook_in_container(
                        "after_run",
                        &container_id,
                        hook,
                        issue,
                        config.hooks.timeout_ms,
                        &hook_cwd,
                    )
                    .await
                    {
                        tracing::warn!(
                            issue_id = %issue.id,
                            issue_identifier = %issue.identifier,
                            error = %err,
                            "after_run hook failure ignored"
                        );
                    }
                }

                let (completion, metrics) = match loop_result {
                    Ok(success) => (
                        WorkerCompletion::Completed {
                            schedule_continuation: success.schedule_continuation,
                        },
                        success.metrics,
                    ),
                    Err(failure) => (
                        WorkerCompletion::Failed {
                            error: failure.error.to_string(),
                        },
                        failure.metrics,
                    ),
                };

                Ok((completion, metrics))
            }
            .await;

        if let Err(err) = docker::stop_container(&container_id).await {
            tracing::warn!(
                issue_id = %issue.id,
                issue_identifier = %issue.identifier,
                container_id = %container_id,
                error = %err,
                "failed to stop docker container cleanly"
            );
        }

        return match docker_result {
            Ok((completion, metrics)) => WorkerResult {
                issue_id,
                completion,
                events: vec![],
                metrics,
            },
            Err(error) => WorkerResult {
                issue_id,
                completion: WorkerCompletion::Failed { error },
                events: vec![],
                metrics: None,
            },
        };
    }

    // 1. Ensure workspace (create dir + after_create hook)
    let hook_cwd = workflow_dir_from_path(&config.workflow_path);
    let workspace_info =
        match workspace::ensure_workspace_for_issue_with_refresh_policy_and_hook_cwd(
            issue,
            &config.workspace,
            &config.hooks,
            config.workspace_refresh_policy,
            &hook_cwd,
        ) {
            Ok(prepared) => prepared.workspace,
            Err(err) => {
                tracing::error!(
                    event = "worker_workspace_failed",
                    issue_id = %issue_id,
                    issue_identifier = %issue.identifier,
                    error = %err,
                    "workspace preparation failed"
                );
                return WorkerResult {
                    issue_id,
                    completion: WorkerCompletion::Failed {
                        error: format!("workspace preparation failed: {err}"),
                    },
                    events: vec![],
                    metrics: None,
                };
            }
        };

    let workspace_path = Path::new(&workspace_info.path);

    // 2. Before-run hook
    if let Err(err) = workspace::run_before_run_hook_for_issue_with_cwd(
        workspace_path,
        &config.hooks,
        issue,
        &hook_cwd,
    ) {
        tracing::error!(
            event = "worker_before_run_failed",
            issue_id = %issue_id,
            error = %err,
            "before_run hook failed"
        );
        return WorkerResult {
            issue_id,
            completion: WorkerCompletion::Failed {
                error: format!("before_run hook failed: {err}"),
            },
            events: vec![],
            metrics: None,
        };
    }

    // 3. Render prompt
    let prompt = match prompt_builder::render_prompt_with_shared_context(
        &config.prompt_template,
        issue,
        attempt,
        config.workspace.base_branch.as_deref(),
        &config.shared_context,
    ) {
        Ok(prompt) => prompt,
        Err(err) => {
            tracing::error!(
                event = "worker_prompt_failed",
                issue_id = %issue_id,
                error = %err,
                "prompt rendering failed"
            );
            return WorkerResult {
                issue_id,
                completion: WorkerCompletion::Failed {
                    error: format!("prompt rendering failed: {err}"),
                },
                events: vec![],
                metrics: None,
            };
        }
    };

    let workspace_root = Path::new(&config.workspace.root);
    let loop_result = match config.agent_backend {
        AgentBackend::Codex => {
            let symphony_bin = std::env::current_exe()
                .ok()
                .map(|path| path.to_string_lossy().to_string());
            let symphony_workflow_path = config.workflow_path.to_string_lossy().to_string();
            let mut session = match app_server::start_session_with_helper_env(
                &config.codex,
                issue,
                workspace_path,
                workspace_root,
                worker_host,
                None,
                app_server::HelperEnv {
                    symphony_bin: symphony_bin.as_deref(),
                    symphony_workflow_path: Some(symphony_workflow_path.as_str()),
                },
            )
            .await
            {
                Ok(session) => session,
                Err(err) => {
                    tracing::error!(
                        event = "worker_session_start_failed",
                        issue_id = %issue_id,
                        issue_identifier = %issue.identifier,
                        error = %err,
                        "codex session start failed"
                    );
                    return WorkerResult {
                        issue_id,
                        completion: WorkerCompletion::Failed {
                            error: format!("codex session start failed: {err}"),
                        },
                        events: vec![],
                        metrics: None,
                    };
                }
            };

            tracing::info!(
                event = "worker_started",
                backend = "codex",
                issue_id = %issue_id,
                issue_identifier = %issue.identifier,
                session_id = %session.session_id,
                workspace_path = %workspace_info.path,
                "worker attempt started"
            );

            let linear_client = crate::linear::client::LinearClient::new(config.tracker.clone());
            let graphql_executor = move |query: String, vars: serde_json::Value| {
                let client = linear_client.clone();
                async move { client.graphql_raw(&query, vars).await }
            };

            let loop_result = run_codex_turns_in_session(
                &mut session,
                issue,
                prompt.clone(),
                config.max_turns,
                &config.tracker,
                graphql_executor,
                {
                    let event_tx = config.event_tx.clone();
                    let issue_id = issue.id.clone();
                    move |event| {
                        let _ = event_tx.send((issue_id.clone(), event));
                    }
                },
            )
            .await;

            if let Err(err) = app_server::stop_session(session).await {
                tracing::warn!(
                    issue_id = %issue_id,
                    error = %err,
                    "failed to stop codex session cleanly"
                );
            }

            loop_result
        }
        AgentBackend::KataCli => {
            let mut session = match rpc_bridge::start_session(
                &config.pi_agent,
                issue,
                workspace_path,
                workspace_root,
                rpc_bridge::StartSessionOptions {
                    worker_host: worker_host.map(ToString::to_string),
                    container_id: None,
                    escalation_tx: config.escalation_tx.clone(),
                    escalation_timeout_ms: config.escalation_timeout_ms,
                    model_override: config.pi_model_override.clone(),
                    symphony_bin: std::env::current_exe()
                        .ok()
                        .map(|path| path.to_string_lossy().to_string()),
                    symphony_workflow_path: Some(
                        config.workflow_path.to_string_lossy().to_string(),
                    ),
                },
            )
            .await
            {
                Ok(session) => session,
                Err(err) => {
                    tracing::error!(
                        event = "worker_session_start_failed",
                        issue_id = %issue_id,
                        issue_identifier = %issue.identifier,
                        error = %err,
                        "pi session start failed"
                    );
                    return WorkerResult {
                        issue_id,
                        completion: WorkerCompletion::Failed {
                            error: format!("pi session start failed: {err}"),
                        },
                        events: vec![],
                        metrics: None,
                    };
                }
            };

            tracing::info!(
                event = "worker_started",
                backend = "pi",
                issue_id = %issue_id,
                issue_identifier = %issue.identifier,
                session_id = %session.session_id,
                workspace_path = %workspace_info.path,
                "worker attempt started"
            );

            let loop_result = run_pi_turns_in_session(
                &mut session,
                issue,
                prompt,
                config.max_turns,
                &config.tracker,
                &mut steer_rx,
                {
                    let event_tx = config.event_tx.clone();
                    let issue_id = issue.id.clone();
                    move |event| {
                        let _ = event_tx.send((issue_id.clone(), event));
                    }
                },
            )
            .await;

            if let Err(err) = rpc_bridge::stop_session(session).await {
                tracing::warn!(
                    issue_id = %issue_id,
                    error = %err,
                    "failed to stop pi session cleanly"
                );
            }

            loop_result
        }
    };

    // 7. After-run hook
    let _ = workspace::run_after_run_hook_for_issue_with_cwd(
        workspace_path,
        &config.hooks,
        issue,
        &hook_cwd,
    );

    // 8. Build result
    match loop_result {
        Ok(success) => WorkerResult {
            issue_id,
            completion: WorkerCompletion::Completed {
                schedule_continuation: success.schedule_continuation,
            },
            events: vec![],
            metrics: success.metrics,
        },
        Err(failure) => WorkerResult {
            issue_id,
            completion: WorkerCompletion::Failed {
                error: failure.error.to_string(),
            },
            events: vec![],
            metrics: failure.metrics,
        },
    }
}

// ── Snapshot Handle (S07 read seam) ─────────────────────────────────────

/// Read-only handle to the latest orchestrator snapshot.
///
/// Clone-cheap (`Arc`-backed). Multiple HTTP handlers can hold references
/// and read concurrently without blocking the orchestrator's mutable loop.
/// The orchestrator publishes a fresh snapshot after every material state
/// change; readers always see a consistent point-in-time view.
#[derive(Clone)]
pub struct SnapshotHandle {
    inner: Arc<RwLock<OrchestratorSnapshot>>,
}

impl SnapshotHandle {
    /// Read the latest published snapshot. Returns a clone so the caller
    /// owns the data without holding the lock.
    pub fn read(&self) -> OrchestratorSnapshot {
        self.inner.read().expect("snapshot rwlock poisoned").clone()
    }

    /// Create a handle pre-loaded with the given snapshot.
    pub fn new(snapshot: OrchestratorSnapshot) -> Self {
        Self {
            inner: Arc::new(RwLock::new(snapshot)),
        }
    }

    /// Publish a new snapshot (called by the orchestrator).
    fn publish(&self, snapshot: OrchestratorSnapshot) {
        *self.inner.write().expect("snapshot rwlock poisoned") = snapshot;
    }
}

// ── Refresh Control Channel (S07 control seam) ──────────────────────────

/// Sender half of the refresh control channel.
///
/// Clone-cheap (`Arc`-backed). HTTP handlers hold this to request an
/// immediate orchestrator tick. Duplicate requests coalesce: if a refresh
/// is already pending, subsequent requests report `coalesced: true` and
/// do not queue additional ticks.
#[derive(Clone)]
pub struct RefreshSender {
    pending: Arc<AtomicBool>,
    notify: Arc<tokio::sync::Notify>,
}

impl RefreshSender {
    /// Request an immediate orchestrator refresh cycle.
    ///
    /// Returns `RefreshRequestOutcome` indicating whether this request was
    /// freshly queued or coalesced with an already-pending request.
    pub fn request_refresh(&self) -> RefreshRequestOutcome {
        let was_pending = self.pending.swap(true, Ordering::SeqCst);
        self.notify.notify_one();
        if was_pending {
            RefreshRequestOutcome {
                queued: false,
                coalesced: true,
                pending_requests: 1,
            }
        } else {
            RefreshRequestOutcome {
                queued: true,
                coalesced: false,
                pending_requests: 1,
            }
        }
    }
}

/// Receiver half of the refresh control channel.
///
/// Only the orchestrator holds this. It checks for pending refresh
/// requests in its runtime loop and clears the flag atomically.
pub struct RefreshReceiver {
    pending: Arc<AtomicBool>,
    notify: Arc<tokio::sync::Notify>,
}

impl RefreshReceiver {
    /// Atomically check and clear the pending refresh flag.
    /// Returns `true` if a refresh was requested since the last check.
    pub fn take_pending(&self) -> bool {
        self.pending.swap(false, Ordering::SeqCst)
    }

    /// Wait until a refresh is requested. This is cancel-safe and suitable
    /// for use inside `tokio::select!`.
    pub async fn notified(&self) {
        self.notify.notified().await;
    }
}

/// Create a paired refresh control channel (sender + receiver).
///
/// The sender is clone-cheap for sharing across HTTP handlers.
/// The receiver should be held by the orchestrator runtime loop.
pub fn refresh_channel() -> (RefreshSender, RefreshReceiver) {
    let pending = Arc::new(AtomicBool::new(false));
    let notify = Arc::new(tokio::sync::Notify::new());
    (
        RefreshSender {
            pending: pending.clone(),
            notify: notify.clone(),
        },
        RefreshReceiver { pending, notify },
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteerResult {
    Delivered {
        issue_id: String,
        issue_identifier: String,
    },
    IssueNotRunning,
    NoActiveSession,
    DeliveryFailed {
        message: String,
    },
}

pub struct SteerDispatch {
    pub issue_identifier: String,
    pub instruction: String,
    pub response_tx: tokio::sync::oneshot::Sender<SteerResult>,
}

#[derive(Clone)]
pub struct SteerSender {
    tx: tokio::sync::mpsc::UnboundedSender<SteerDispatch>,
}

impl SteerSender {
    pub async fn request_steer(
        &self,
        issue_identifier: String,
        instruction: String,
        timeout: Duration,
    ) -> SteerResult {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        if self
            .tx
            .send(SteerDispatch {
                issue_identifier,
                instruction,
                response_tx,
            })
            .is_err()
        {
            return SteerResult::DeliveryFailed {
                message: "orchestrator_unavailable".to_string(),
            };
        }

        match tokio::time::timeout(timeout, response_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => SteerResult::DeliveryFailed {
                message: "orchestrator_dropped_response".to_string(),
            },
            Err(_) => SteerResult::DeliveryFailed {
                message: "steer_timeout".to_string(),
            },
        }
    }
}

pub fn steer_channel() -> (
    SteerSender,
    tokio::sync::mpsc::UnboundedReceiver<SteerDispatch>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (SteerSender { tx }, rx)
}

pub const CONTINUATION_RETRY_DELAY_MS: i64 = 1_000;
pub const FAILURE_RETRY_BASE_MS: i64 = 10_000;
/// Marker included in stall-induced failure strings.
///
/// `detect_stalled_workers` appends this marker to synthetic failure messages,
/// and `handle_worker_completion` checks for it so stall-induced failures are
/// not treated as generic `failed` notification events.
const STALL_FAILURE_MARKER: &str = "without agent activity";
const MAX_STEER_DISPATCHES_PER_TICK: usize = 4;
const STEER_FOLLOW_UP_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryKind {
    Continuation,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeEvent {
    StartupCleanup,
    Reconcile,
    Validate,
    Dispatch,
    ValidationSkippedDispatch,
    RetryScheduled {
        issue_id: String,
        attempt: u32,
        due_at_ms: i64,
        token: String,
        retry_kind: RetryKind,
    },
    RetryIgnoredStale {
        issue_id: String,
        token: String,
    },
    WorkerCompleted {
        issue_id: String,
        issue_identifier: String,
        session_id: Option<String>,
    },
    WorkerFailed {
        issue_id: String,
        issue_identifier: String,
        session_id: Option<String>,
        error: String,
    },
    WorkspacePrepareFailed {
        issue_id: String,
        issue_identifier: String,
        error: String,
    },
    WorkerStalled {
        issue_id: String,
        issue_identifier: String,
        session_id: Option<String>,
        elapsed_ms: i64,
    },
    SteerReceived {
        issue_identifier: String,
        instruction_preview: String,
    },
    SteerDelivered {
        issue_id: String,
        issue_identifier: String,
    },
    SteerFailed {
        issue_identifier: String,
        error: String,
    },
    /// An HTTP refresh request was received and will trigger an immediate tick.
    RefreshRequested,
    /// An HTTP refresh request was received but coalesced with an already-pending
    /// refresh (no additional tick needed).
    RefreshCoalesced,
}

#[derive(Debug, Clone)]
pub struct DispatchedIssue {
    pub issue: Issue,
    pub attempt: Option<u32>,
    pub worker_host: Option<String>,
    pub workspace_refresh_policy: workspace::ExistingWorkspaceRefreshPolicy,
    pub workspace_status_context: Option<String>,
}

#[derive(Debug, Clone)]
struct WorkspaceDispatchPreparation {
    path: String,
    status_context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TickResult {
    pub dispatched_issue_ids: Vec<String>,
    pub dispatched_issues: Vec<DispatchedIssue>,
    pub dispatch_skipped: bool,
}

/// Result sent back from a spawned worker task to the orchestrator loop.
#[derive(Debug)]
pub struct WorkerResult {
    pub issue_id: String,
    pub completion: WorkerCompletion,
    pub events: Vec<AgentEvent>,
    pub metrics: Option<TurnMetrics>,
}

#[derive(Debug, Clone)]
struct PendingTerminalCleanup {
    issue: Issue,
    workspace_path: String,
}

#[derive(Debug, Clone)]
pub struct TurnMetrics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub rate_limits: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct RetryContext {
    pub worker_host: Option<String>,
    pub workspace_path: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct RunningSessionStats {
    turn_count: u32,
    last_activity_at: Option<DateTime<Utc>>,
    total_tokens: u64,
    last_event: Option<String>,
    last_event_message: Option<String>,
    session_id: Option<String>,
    /// Name of the tool currently executing (set on tool_start, cleared on tool_end).
    current_tool_name: Option<String>,
    /// Short preview of arguments for the currently executing tool.
    current_tool_args_preview: Option<String>,
}

#[derive(Debug, Clone)]
struct CompletionCommentSummary {
    turn_count: u32,
    total_tokens: u64,
    duration: chrono::Duration,
    worker_host: Option<String>,
}

#[derive(Debug, Clone)]
pub enum WorkerCompletion {
    Completed { schedule_continuation: bool },
    Failed { error: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscalationResolveResult {
    Resolved,
    NotFound,
    AlreadyResolved,
}

struct EscalationEntry {
    request: EscalationRequest,
    response_tx: tokio::sync::oneshot::Sender<EscalationResponse>,
}

const ESCALATION_RESOLVED_CACHE_CAPACITY: usize = 1_024;

#[derive(Default)]
struct ResolvedEscalationCache {
    ids: HashSet<String>,
    order: VecDeque<String>,
}

impl ResolvedEscalationCache {
    fn contains(&self, request_id: &str) -> bool {
        self.ids.contains(request_id)
    }

    fn insert(&mut self, request_id: String) {
        if self.ids.contains(&request_id) {
            return;
        }

        self.ids.insert(request_id.clone());
        self.order.push_back(request_id);

        while self.order.len() > ESCALATION_RESOLVED_CACHE_CAPACITY {
            if let Some(evicted) = self.order.pop_front() {
                self.ids.remove(&evicted);
            }
        }
    }

    fn remove(&mut self, request_id: &str) {
        if !self.ids.remove(request_id) {
            return;
        }

        if let Some(position) = self.order.iter().position(|entry| entry == request_id) {
            self.order.remove(position);
        }
    }
}

#[derive(Default)]
struct EscalationRegistryState {
    pending: HashMap<String, EscalationEntry>,
    resolved: ResolvedEscalationCache,
}

#[derive(Clone, Default)]
pub struct EscalationRegistry {
    state: Arc<Mutex<EscalationRegistryState>>,
}

impl EscalationRegistry {
    pub fn register(
        &self,
        request: EscalationRequest,
        response_tx: tokio::sync::oneshot::Sender<EscalationResponse>,
    ) {
        let mut state = self
            .state
            .lock()
            .expect("escalation registry mutex poisoned");
        state.resolved.remove(&request.id);
        state.pending.insert(
            request.id.clone(),
            EscalationEntry {
                request,
                response_tx,
            },
        );
    }

    pub fn resolve(
        &self,
        request_id: &str,
        response: EscalationResponse,
    ) -> EscalationResolveResult {
        let entry = {
            let mut state = self
                .state
                .lock()
                .expect("escalation registry mutex poisoned");

            if let Some(entry) = state.pending.remove(request_id) {
                entry
            } else if state.resolved.contains(request_id) {
                return EscalationResolveResult::AlreadyResolved;
            } else {
                return EscalationResolveResult::NotFound;
            }
        };

        if entry.response_tx.send(response).is_ok() {
            let mut state = self
                .state
                .lock()
                .expect("escalation registry mutex poisoned");
            state.resolved.insert(request_id.to_string());
            EscalationResolveResult::Resolved
        } else {
            EscalationResolveResult::NotFound
        }
    }

    pub fn remove(&self, request_id: &str) -> bool {
        let mut state = self
            .state
            .lock()
            .expect("escalation registry mutex poisoned");
        state.pending.remove(request_id).is_some()
    }

    pub fn cancel_for_issue(&self, issue_id: &str) -> Vec<EscalationRequest> {
        let mut state = self
            .state
            .lock()
            .expect("escalation registry mutex poisoned");

        let ids_to_remove: Vec<String> = state
            .pending
            .iter()
            .filter_map(|(request_id, entry)| {
                if entry.request.issue_id == issue_id {
                    Some(request_id.clone())
                } else {
                    None
                }
            })
            .collect();

        let mut cancelled = Vec::with_capacity(ids_to_remove.len());
        for request_id in ids_to_remove {
            if let Some(entry) = state.pending.remove(&request_id) {
                cancelled.push(entry.request);
            }
        }

        cancelled
    }

    pub fn pending_snapshot(&self) -> Vec<PendingEscalation> {
        let state = self
            .state
            .lock()
            .expect("escalation registry mutex poisoned");

        let mut pending: Vec<PendingEscalation> = state
            .pending
            .values()
            .map(|entry| PendingEscalation {
                request_id: entry.request.id.clone(),
                issue_id: entry.request.issue_id.clone(),
                issue_identifier: entry.request.issue_identifier.clone(),
                method: entry.request.method.clone(),
                preview: escalation_preview(&entry.request.payload),
                created_at: entry.request.created_at,
                timeout_ms: entry.request.timeout_ms,
            })
            .collect();

        pending.sort_by_key(|entry| entry.created_at);
        pending
    }
}

fn escalation_preview(payload: &serde_json::Value) -> String {
    let preview = payload
        .get("questions")
        .and_then(|questions| questions.as_array())
        .and_then(|questions| questions.first())
        .and_then(|question| {
            question
                .get("question")
                .and_then(|question_text| question_text.as_str())
                .or_else(|| {
                    question
                        .get("prompt")
                        .and_then(|question_text| question_text.as_str())
                })
        })
        .map(str::to_string)
        .or_else(|| {
            payload
                .get("question")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .or_else(|| {
            payload
                .get("prompt")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| truncate_for_display(&payload.to_string(), 120));

    truncate_for_display(&preview, 120)
}

pub trait OrchestratorPort {
    fn startup_terminal_issues(&mut self, terminal_states: &[String]) -> Result<Vec<Issue>>;

    fn reconcile_running_issues(&mut self, running_issue_ids: &[String]) -> Result<Vec<Issue>>;

    fn validate_dispatch_preflight(&mut self, config: &ServiceConfig) -> Result<()>;

    fn fetch_candidate_issues(&mut self) -> Result<Vec<Issue>>;

    fn refresh_issue(&mut self, issue_id: &str) -> Result<Option<Issue>>;

    /// Create a tracker comment on an issue.
    fn create_issue_comment(&mut self, issue_id: &str, body: &str) -> Result<()>;

    /// Update an issue's workflow state in the tracker (e.g., move to "In Progress").
    fn update_issue_state(&mut self, issue_id: &str, state_name: &str) -> Result<()>;
}

/// S06 runtime authority loop state.
///
/// The orchestrator is the single mutable owner of dispatch/reconcile/retry
/// state in this process. State mutation only happens through `&mut self`
/// methods (startup cleanup, tick, retry handlers).
pub struct Orchestrator {
    workflow_store: Option<Arc<WorkflowStore>>,
    config: ServiceConfig,
    server_port_override: Option<u16>,
    state: OrchestratorState,
    events: Vec<RuntimeEvent>,
    retry_tokens: HashMap<String, String>,
    worker_last_activity_ms: HashMap<String, i64>,
    worker_session_info: HashMap<String, WorkerSessionInfo>,
    worker_session_ids: HashMap<String, String>,
    worker_steer_tx:
        HashMap<String, tokio::sync::mpsc::UnboundedSender<rpc_bridge::FollowUpRequest>>,
    running_session_stats: HashMap<String, RunningSessionStats>,
    completion_comment_summaries: HashMap<String, CompletionCommentSummary>,
    /// Blocked issues from the latest dispatch phase.
    blocked_issues: Vec<crate::domain::BlockedIssueEntry>,
    pending_terminal_cleanup: HashMap<String, PendingTerminalCleanup>,
    /// Normalized running issue state cache used for per-state slot accounting.
    running_issue_states: HashMap<String, String>,
    /// Parent identifier cache for currently running issues (if provided by tracker).
    running_parent_identifiers: HashMap<String, Option<String>>,
    next_retry_token: u64,
    poll_count: u64,
    last_poll_at: Option<DateTime<Utc>>,
    /// Optional shared snapshot handle for HTTP read access.
    snapshot_handle: Option<SnapshotHandle>,
    /// Optional shared event hub for websocket stream publication.
    event_hub: Option<EventHub>,
    /// Optional refresh receiver for HTTP control access.
    refresh_receiver: Option<RefreshReceiver>,
    /// Channel for receiving results from spawned worker tasks.
    worker_result_rx: tokio::sync::mpsc::UnboundedReceiver<WorkerResult>,
    /// Sender half cloned into each spawned worker task.
    worker_result_tx: tokio::sync::mpsc::UnboundedSender<WorkerResult>,
    /// Channel for receiving streamed worker events from spawned worker tasks.
    worker_event_rx: tokio::sync::mpsc::UnboundedReceiver<(String, AgentEvent)>,
    /// Sender half cloned into each spawned worker task for event streaming.
    worker_event_tx: tokio::sync::mpsc::UnboundedSender<(String, AgentEvent)>,
    /// Channel for receiving escalation registrations from RPC bridge sessions.
    worker_escalation_rx: tokio::sync::mpsc::UnboundedReceiver<rpc_bridge::EscalationDispatch>,
    /// Sender half cloned into each spawned worker task for escalation registration.
    worker_escalation_tx: tokio::sync::mpsc::UnboundedSender<rpc_bridge::EscalationDispatch>,
    steer_sender: SteerSender,
    steer_rx: tokio::sync::mpsc::UnboundedReceiver<SteerDispatch>,
    escalation_registry: EscalationRegistry,
    /// Shared ephemeral cross-worker context store.
    shared_context_store: SharedContextStore,
    /// Optional supervisor lifecycle controller.
    supervisor_agent: Option<SupervisorAgent>,
    /// Optional A1 triage runtime (preview/automatic factory stage).
    triage_runtime: Option<TriageRuntime>,
    /// The prompt template from the WORKFLOW.md body, used to render per-issue prompts.
    prompt_template: String,
    /// Resolved workflow path used by worker helpers and workflow-relative paths.
    workflow_path: PathBuf,
}

impl Orchestrator {
    pub fn new_with_workflow_store(workflow_store: Arc<WorkflowStore>) -> Self {
        Self::new_with_workflow_store_and_port_override(workflow_store, None)
    }

    pub fn new_with_workflow_store_and_port_override(
        workflow_store: Arc<WorkflowStore>,
        server_port_override: Option<u16>,
    ) -> Self {
        let (workflow_def, config) = workflow_store.effective_config();
        Self::from_runtime_config(
            config,
            workflow_def.prompt_template,
            Some(workflow_store),
            server_port_override,
        )
    }

    pub fn new(config: ServiceConfig, prompt_template: String) -> Self {
        Self::from_runtime_config(config, prompt_template, None, None)
    }

    fn from_runtime_config(
        config: ServiceConfig,
        prompt_template: String,
        workflow_store: Option<Arc<WorkflowStore>>,
        server_port_override: Option<u16>,
    ) -> Self {
        let poll_interval_ms = config.polling.interval_ms;
        let max_concurrent_agents = config.agent.max_concurrent_agents;
        let shared_context_ttl_ms = config.shared_context.ttl_ms;
        let shared_context_max_entries = config.shared_context.max_entries;
        let workflow_path = workflow_store
            .as_ref()
            .map(|store| store.workflow_path().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("WORKFLOW.md"));
        let (worker_result_tx, worker_result_rx) = tokio::sync::mpsc::unbounded_channel();
        let (worker_event_tx, worker_event_rx) = tokio::sync::mpsc::unbounded_channel();
        let (worker_escalation_tx, worker_escalation_rx) = tokio::sync::mpsc::unbounded_channel();
        let (steer_sender, steer_rx) = steer_channel();

        Self {
            workflow_store,
            config,
            server_port_override,
            state: OrchestratorState {
                poll_interval_ms,
                max_concurrent_agents,
                running: HashMap::new(),
                claimed: std::collections::HashSet::new(),
                retry_attempts: HashMap::new(),
                completed: HashMap::new(),
                codex_totals: CodexTotals::default(),
                codex_rate_limits: None,
            },
            events: vec![],
            retry_tokens: HashMap::new(),
            worker_last_activity_ms: HashMap::new(),
            worker_session_info: HashMap::new(),
            worker_session_ids: HashMap::new(),
            worker_steer_tx: HashMap::new(),
            running_session_stats: HashMap::new(),
            completion_comment_summaries: HashMap::new(),
            blocked_issues: Vec::new(),
            pending_terminal_cleanup: HashMap::new(),
            running_issue_states: HashMap::new(),
            running_parent_identifiers: HashMap::new(),
            next_retry_token: 0,
            poll_count: 0,
            last_poll_at: None,
            snapshot_handle: None,
            event_hub: None,
            refresh_receiver: None,
            worker_result_rx,
            worker_result_tx,
            worker_event_rx,
            worker_event_tx,
            worker_escalation_rx,
            worker_escalation_tx,
            steer_sender,
            steer_rx,
            escalation_registry: EscalationRegistry::default(),
            shared_context_store: SharedContextStore::new(
                shared_context_ttl_ms,
                shared_context_max_entries,
            ),
            supervisor_agent: None,
            triage_runtime: None,
            prompt_template,
            workflow_path,
        }
    }

    fn refresh_runtime_config(&mut self) {
        if let Some(workflow_store) = self.workflow_store.as_ref() {
            let (workflow_def, config) = workflow_store.effective_config();
            self.config = config;
            self.prompt_template = workflow_def.prompt_template;
            self.workflow_path = workflow_store.workflow_path().to_path_buf();
        }

        if let Some(port) = self.server_port_override {
            self.config.server.port = Some(port);
        }

        if self.config.shared_context.ttl_ms > 0 && self.config.shared_context.max_entries > 0 {
            self.shared_context_store.update_settings(
                self.config.shared_context.ttl_ms,
                self.config.shared_context.max_entries,
            );
        } else {
            tracing::warn!(
                event = "shared_context_config_invalid",
                ttl_ms = self.config.shared_context.ttl_ms,
                max_entries = self.config.shared_context.max_entries,
                "skipping shared context store settings update because workflow config is invalid"
            );
        }

        self.state.max_concurrent_agents = self.config.agent.max_concurrent_agents;
        self.state.poll_interval_ms = self.config.polling.interval_ms;
    }

    fn queue_slack_notification(
        &self,
        event_type: &str,
        issue_identifier: &str,
        issue_title: &str,
        message: &str,
        issue_url: Option<&str>,
    ) {
        let Some(slack_config) = self
            .config
            .notifications
            .as_ref()
            .and_then(|notifications| notifications.slack.as_ref())
            .cloned()
        else {
            return;
        };

        if !notifications::should_notify(&slack_config, event_type) {
            return;
        }

        let issue_identifier = issue_identifier.to_string();
        let issue_title = issue_title.to_string();
        let event_type = event_type.to_string();
        let message = message.to_string();
        let issue_url = issue_url.map(String::from);

        if let Ok(runtime_handle) = tokio::runtime::Handle::try_current() {
            runtime_handle.spawn(async move {
                if let Err(err) = notifications::send_slack_notification(
                    &slack_config,
                    &event_type,
                    &issue_identifier,
                    &issue_title,
                    &message,
                    issue_url.as_deref(),
                )
                .await
                {
                    tracing::warn!(
                        event = "notification_failed",
                        issue_identifier = %issue_identifier,
                        event_type = %event_type,
                        error = %err,
                        webhook_url = "[REDACTED]",
                        "failed to send Slack notification"
                    );
                }
            });
        } else {
            tracing::warn!(
                event = "notification_failed",
                issue_identifier = %issue_identifier,
                event_type = %event_type,
                error = "tokio runtime unavailable",
                "skipping Slack notification because no tokio runtime is active"
            );
        }
    }

    pub async fn run(&mut self, port: &mut dyn OrchestratorPort) -> Result<()> {
        self.startup_cleanup(port)?;
        self.sync_supervisor_lifecycle().await;
        self.publish_snapshot();
        let mut next_poll_due = tokio::time::Instant::now();
        let mut tick_requested = true;

        loop {
            let now = tokio::time::Instant::now();
            if tick_requested || now >= next_poll_due {
                self.refresh_runtime_config();
                self.sync_supervisor_lifecycle().await;

                let now_ms = Utc::now().timestamp_millis();
                let stall_timeout_ms =
                    backend_stall_timeout_ms(&self.config, self.config.agent_backend);

                self.detect_stalled_workers(now_ms, stall_timeout_ms);

                if let Some(runtime) = self.triage_runtime.as_mut() {
                    match runtime.poll(&self.config).await {
                        Ok(summary) => {
                            if summary.triage_enabled
                                || summary.reconciled_intents > 0
                                || summary.issues_seen > 0
                            {
                                tracing::info!(
                                    event = "triage_poll_completed",
                                    enabled = summary.triage_enabled,
                                    issues_seen = summary.issues_seen,
                                    attempts_started = summary.attempts_started,
                                    attempts_completed = summary.attempts_completed,
                                    attempts_failed = summary.attempts_failed,
                                    ineligible = summary.ineligible,
                                    skipped = summary.skipped,
                                    reconciled_intents = summary.reconciled_intents,
                                    "triage poll completed"
                                );
                            }
                        }
                        Err(err) => {
                            tracing::warn!(
                                event = "triage_poll_failed",
                                error = %err,
                                "triage poll failed; continuing orchestrator loop"
                            );
                        }
                    }
                }

                match self.tick_with_refresh(port, false) {
                    Ok(tick_result) => {
                        self.spawn_workers_for_dispatched(&tick_result.dispatched_issues, port);
                    }
                    Err(err) => {
                        tracing::warn!(
                            phase = "tick",
                            error = %err,
                            "orchestrator tick failed; continuing"
                        );
                    }
                }

                let retry_dispatched =
                    self.process_due_retries(port, Utc::now().timestamp_millis());
                self.spawn_workers_for_dispatched(&retry_dispatched, port);
                self.publish_snapshot();

                tick_requested = false;
                next_poll_due = tokio::time::Instant::now()
                    + Duration::from_millis(self.state.poll_interval_ms);
            }

            // Sleep until next poll deadline, but wake early on refresh request
            // or worker channels.
            let refresh_notify = self.refresh_receiver.as_ref().map(|r| r.notify.clone());

            tokio::select! {
                _ = tokio::time::sleep_until(next_poll_due) => {
                    tick_requested = true;
                },
                event = self.worker_event_rx.recv() => {
                    if let Some((issue_id, event)) = event {
                        self.ingest_agent_event(&issue_id, &event);
                        self.drain_ready_worker_events();
                        self.publish_snapshot();
                    }
                },
                escalation = self.worker_escalation_rx.recv() => {
                    if let Some(dispatch) = escalation {
                        self.handle_escalation_dispatch(dispatch);
                        self.publish_snapshot();
                    }
                    while let Ok(dispatch) = self.worker_escalation_rx.try_recv() {
                        self.handle_escalation_dispatch(dispatch);
                        self.publish_snapshot();
                    }
                },
                steer = self.steer_rx.recv() => {
                    if let Some(dispatch) = steer {
                        self.handle_steer_dispatch(dispatch).await;
                        self.publish_snapshot();

                        let mut drained = 1usize;
                        while drained < MAX_STEER_DISPATCHES_PER_TICK {
                            match self.steer_rx.try_recv() {
                                Ok(dispatch) => {
                                    self.handle_steer_dispatch(dispatch).await;
                                    self.publish_snapshot();
                                    drained = drained.saturating_add(1);
                                }
                                Err(_) => break,
                            }
                        }
                    }
                },
                result = self.worker_result_rx.recv() => {
                    if let Some(result) = result {
                        self.drain_ready_worker_events();
                        self.handle_worker_result(result);
                        self.publish_snapshot();
                    }
                    // Drain any additional ready results.
                    while let Ok(result) = self.worker_result_rx.try_recv() {
                        self.drain_ready_worker_events();
                        self.handle_worker_result(result);
                        self.publish_snapshot();
                    }
                    if self.drain_ready_worker_events() > 0 {
                        self.publish_snapshot();
                    }
                },
                _ = async {
                    if let Some(notify) = &refresh_notify {
                        notify.notified().await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {
                    if let Some(receiver) = &self.refresh_receiver {
                        if receiver.take_pending() {
                            tracing::info!(
                                event = "refresh_requested",
                                "HTTP refresh request woke orchestrator loop; triggering immediate tick"
                            );
                            self.emit_runtime_event(RuntimeEvent::RefreshRequested);
                            tick_requested = true;
                        }
                    }
                },
            }
        }
    }

    /// Spawn a tokio task for each newly dispatched issue.
    fn spawn_workers_for_dispatched(
        &mut self,
        dispatched: &[DispatchedIssue],
        port: &mut dyn OrchestratorPort,
    ) {
        for d in dispatched {
            let mut issue = d.issue.clone();
            let mut effective_state = issue.state.clone();

            // Only move "Todo" issues to "In Progress" on dispatch.
            // Other active states (Agent Review, Merging, Rework, In Progress)
            // are preserved so the agent sees the correct state and follows
            // the matching workflow in WORKFLOW.md Step 0.
            if normalize_issue_state(&issue.state) == "todo" {
                match port.update_issue_state(&issue.id, "In Progress") {
                    Ok(()) => {
                        effective_state = "In Progress".to_string();
                    }
                    Err(err) => {
                        tracing::warn!(
                            event = "state_transition_failed",
                            tracker_kind = %self.config.tracker.kind.as_deref().unwrap_or("unknown"),
                            issue_id = %issue.id,
                            issue_identifier = %issue.identifier,
                            target_state = "In Progress",
                            error = %err,
                            "failed to move issue to In Progress; continuing with dispatch"
                        );
                    }
                }
            }

            issue.state = effective_state.clone();
            self.running_issue_states
                .insert(issue.id.clone(), effective_state.clone());

            // Update status from "scheduled" to "running" and record the
            // tracker state the worker is actually dispatched with.
            if let Some(attempt) = self.state.running.get_mut(&issue.id) {
                attempt.status = "running".to_string();
                attempt.tracker_state = Some(effective_state.clone());
            }
            let attempt = d.attempt;
            let worker_host = d.worker_host.clone();
            let tx = self.worker_result_tx.clone();
            self.worker_steer_tx.remove(&issue.id);

            let mut steer_rx = None;
            if self.config.agent_backend == AgentBackend::KataCli {
                let (steer_tx, rx) = tokio::sync::mpsc::unbounded_channel();
                self.worker_steer_tx.insert(issue.id.clone(), steer_tx);
                steer_rx = Some(rx);
            }

            let prompt_template = Self::ensure_shared_context_placeholder(
                self.resolve_prompt_for_state(&effective_state),
            );
            let shared_context = Self::append_workspace_status_context(
                self.build_shared_context_block_for_issue(&issue),
                d.workspace_status_context.as_deref(),
            );
            let pi_model_override = if self.config.agent_backend == AgentBackend::KataCli {
                effective_pi_model_for_issue(&self.config, &issue)
            } else {
                None
            };

            if let Some(run_attempt) = self.state.running.get_mut(&issue.id) {
                run_attempt.model = pi_model_override.clone();
            }

            let workflow_path = self.workflow_path.clone();

            let task_config = WorkerTaskConfig {
                workspace: self.config.workspace.clone(),
                hooks: self.config.hooks.clone(),
                codex: self.config.codex.clone(),
                pi_agent: self.config.pi_agent.clone(),
                pi_model_override,
                agent_backend: self.config.agent_backend,
                max_turns: self.config.agent.max_turns,
                tracker: self.config.tracker.clone(),
                prompt_template,
                shared_context,
                workspace_refresh_policy: d.workspace_refresh_policy,
                event_tx: self.worker_event_tx.clone(),
                escalation_tx: self.worker_escalation_tx.clone(),
                escalation_timeout_ms: self.config.agent.escalation_timeout_ms,
                workflow_path,
            };

            tokio::spawn(async move {
                let result = run_worker_task(
                    &issue,
                    attempt,
                    worker_host.as_deref(),
                    &task_config,
                    steer_rx,
                )
                .await;

                if let Err(err) = tx.send(result) {
                    tracing::error!(
                        error = %err,
                        "failed to send worker result back to orchestrator"
                    );
                }
            });
        }
    }

    /// Process a worker result received from a spawned worker task.
    fn handle_worker_result(&mut self, result: WorkerResult) {
        // Ingest agent events (for activity tracking, token accounting, etc.)
        for event in &result.events {
            self.ingest_agent_event(&result.issue_id, event);
        }

        // WorkerResult.metrics is retained as a completion summary payload.
        if let Some(metrics) = &result.metrics {
            tracing::info!(
                event = "worker_result_metrics_summary",
                issue_id = %result.issue_id,
                input_tokens = metrics.input_tokens,
                output_tokens = metrics.output_tokens,
                total_tokens = metrics.total_tokens,
                has_rate_limits = metrics.rate_limits.is_some(),
                "received worker result metrics summary"
            );
        }

        // Handle completion (schedules retry on failure, marks complete on success)
        self.handle_worker_completion(
            &result.issue_id,
            result.completion,
            Utc::now().timestamp_millis(),
        );
    }

    /// Drain any worker events already queued by spawned worker tasks.
    ///
    /// Returns the number of events ingested.
    fn drain_ready_worker_events(&mut self) -> usize {
        let mut drained = 0usize;
        while let Ok((issue_id, event)) = self.worker_event_rx.try_recv() {
            self.ingest_agent_event(&issue_id, &event);
            drained = drained.saturating_add(1);
        }
        drained
    }

    fn handle_escalation_dispatch(&mut self, mut dispatch: rpc_bridge::EscalationDispatch) {
        let issue_state = self
            .running_issue_states
            .get(&dispatch.request.issue_id)
            .cloned()
            .or_else(|| {
                self.state
                    .running
                    .get(&dispatch.request.issue_id)
                    .and_then(|attempt| attempt.tracker_state.clone())
            });
        let parent_identifier = self
            .running_parent_identifiers
            .get(&dispatch.request.issue_id)
            .cloned()
            .flatten();

        enrich_escalation_payload(
            &mut dispatch.request.payload,
            &dispatch.request.issue_identifier,
            issue_state.as_deref(),
            parent_identifier.as_deref(),
        );

        let request_id = dispatch.request.id.clone();
        self.escalation_registry
            .register(dispatch.request.clone(), dispatch.response_tx);

        tracing::info!(
            event = "escalation_registered",
            issue_id = %dispatch.request.issue_id,
            issue_identifier = %dispatch.request.issue_identifier,
            issue_state = %issue_state.unwrap_or_default(),
            request_id = %request_id,
            method = %dispatch.request.method,
            "registered pending escalation"
        );
    }

    async fn handle_steer_dispatch(&mut self, dispatch: SteerDispatch) {
        let issue_identifier = dispatch.issue_identifier.trim().to_ascii_uppercase();
        let instruction = dispatch.instruction.trim().to_string();
        let instruction_preview = truncate_for_display(&instruction, 100);

        self.emit_runtime_event(RuntimeEvent::SteerReceived {
            issue_identifier: issue_identifier.clone(),
            instruction_preview: instruction_preview.clone(),
        });

        tracing::info!(
            event = "steer_received",
            issue_identifier = %issue_identifier,
            instruction_preview = %instruction_preview,
            "received operator steer request"
        );

        let response = if instruction.is_empty() {
            SteerResult::DeliveryFailed {
                message: "instruction_empty".to_string(),
            }
        } else {
            let running_match = self
                .state
                .running
                .iter()
                .find(|(_, run_attempt)| {
                    run_attempt
                        .issue_identifier
                        .eq_ignore_ascii_case(&issue_identifier)
                })
                .map(|(issue_id, run_attempt)| {
                    (issue_id.clone(), run_attempt.issue_identifier.clone())
                });

            match running_match {
                None => SteerResult::IssueNotRunning,
                Some((issue_id, canonical_identifier)) => {
                    if !self.worker_session_ids.contains_key(&issue_id) {
                        SteerResult::NoActiveSession
                    } else if let Some(steer_tx) = self.worker_steer_tx.get(&issue_id).cloned() {
                        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                        if steer_tx
                            .send(rpc_bridge::FollowUpRequest {
                                instruction,
                                response_tx,
                            })
                            .is_err()
                        {
                            self.worker_steer_tx.remove(&issue_id);
                            SteerResult::NoActiveSession
                        } else {
                            match tokio::time::timeout(STEER_FOLLOW_UP_TIMEOUT, response_rx).await {
                                Ok(Ok(Ok(()))) => SteerResult::Delivered {
                                    issue_id,
                                    issue_identifier: canonical_identifier,
                                },
                                Ok(Ok(Err(error))) => {
                                    SteerResult::DeliveryFailed { message: error }
                                }
                                Ok(Err(_)) => SteerResult::DeliveryFailed {
                                    message: "worker_follow_up_response_dropped".to_string(),
                                },
                                Err(_) => SteerResult::DeliveryFailed {
                                    message: "worker_follow_up_timeout".to_string(),
                                },
                            }
                        }
                    } else {
                        SteerResult::NoActiveSession
                    }
                }
            }
        };

        match &response {
            SteerResult::Delivered {
                issue_id,
                issue_identifier,
            } => {
                self.emit_runtime_event(RuntimeEvent::SteerDelivered {
                    issue_id: issue_id.clone(),
                    issue_identifier: issue_identifier.clone(),
                });
                tracing::info!(
                    event = "steer_delivered",
                    issue_id = %issue_id,
                    issue_identifier = %issue_identifier,
                    "delivered steer instruction to running worker"
                );
            }
            SteerResult::IssueNotRunning => {
                self.emit_runtime_event(RuntimeEvent::SteerFailed {
                    issue_identifier: issue_identifier.clone(),
                    error: "issue_not_running".to_string(),
                });
                tracing::warn!(
                    event = "steer_failed",
                    issue_identifier = %issue_identifier,
                    error = "issue_not_running",
                    "cannot steer issue because it is not running"
                );
            }
            SteerResult::NoActiveSession => {
                self.emit_runtime_event(RuntimeEvent::SteerFailed {
                    issue_identifier: issue_identifier.clone(),
                    error: "no_active_session".to_string(),
                });
                tracing::warn!(
                    event = "steer_failed",
                    issue_identifier = %issue_identifier,
                    error = "no_active_session",
                    "cannot steer issue because no active rpc session is available"
                );
            }
            SteerResult::DeliveryFailed { message } => {
                self.emit_runtime_event(RuntimeEvent::SteerFailed {
                    issue_identifier: issue_identifier.clone(),
                    error: message.clone(),
                });
                tracing::warn!(
                    event = "steer_failed",
                    issue_identifier = %issue_identifier,
                    error = %message,
                    "steer instruction delivery failed"
                );
            }
        }

        let _ = dispatch.response_tx.send(response);
    }

    pub fn resolve_escalation(
        &self,
        request_id: &str,
        response: serde_json::Value,
        responder_id: Option<String>,
    ) -> EscalationResolveResult {
        let escalation_response = EscalationResponse {
            request_id: request_id.to_string(),
            response,
            responder_id,
            responded_at: Utc::now(),
        };

        self.escalation_registry
            .resolve(request_id, escalation_response)
    }

    pub fn escalation_registry(&self) -> EscalationRegistry {
        self.escalation_registry.clone()
    }

    pub fn startup_cleanup(&mut self, port: &mut dyn OrchestratorPort) -> Result<()> {
        self.emit_runtime_event(RuntimeEvent::StartupCleanup);
        tracing::info!(
            phase = "startup_cleanup",
            "running startup terminal cleanup"
        );

        let terminal_issues = port.startup_terminal_issues(&self.config.tracker.terminal_states)?;
        let workspace_root = Path::new(&self.config.workspace.root);
        let startup_workspaces =
            workspace::scan_workspace_root(workspace_root, &self.config.workspace.branch_prefix);

        tracing::info!(
            event = "startup_workspace_scan",
            root_path = %workspace_root.display(),
            workspace_count = startup_workspaces.len(),
            "completed startup workspace scan"
        );

        for issue in terminal_issues {
            let workspace_path_hint = startup_workspaces
                .get(&issue.identifier)
                .map(|path| path.to_string_lossy().to_string());

            self.mark_issue_terminal(&issue, workspace_path_hint.as_deref(), false);

            if let Some(workspace_path) = workspace_path_hint {
                let workspace_path_ref = Path::new(&workspace_path);
                if !workspace_path_ref.exists() {
                    tracing::info!(
                        event = "startup_orphan_workspace_removed",
                        issue_identifier = %issue.identifier,
                        workspace_path = %workspace_path,
                        "orphan workspace no longer present after startup cleanup"
                    );
                }
            }
        }

        Ok(())
    }

    pub fn tick(&mut self, port: &mut dyn OrchestratorPort) -> Result<TickResult> {
        self.tick_with_refresh(port, true)
    }

    fn tick_with_refresh(
        &mut self,
        port: &mut dyn OrchestratorPort,
        refresh_runtime_config: bool,
    ) -> Result<TickResult> {
        self.poll_count += 1;
        self.last_poll_at = Some(Utc::now());
        self.blocked_issues.clear();

        if refresh_runtime_config {
            self.refresh_runtime_config();
        }

        let pruned_entries = self.prune_expired_shared_context(Utc::now());
        if pruned_entries > 0 {
            tracing::info!(
                event = "shared_context_pruned",
                pruned_entries,
                "pruned expired shared context entries"
            );
        }

        self.emit_runtime_event(RuntimeEvent::Reconcile);
        tracing::info!(phase = "reconcile", "starting orchestrator tick phase");
        self.reconcile_running(port)?;

        self.emit_runtime_event(RuntimeEvent::Validate);
        tracing::info!(phase = "validate", "starting orchestrator tick phase");

        if let Err(err) = config::validate(&self.config) {
            tracing::warn!(
                phase = "dispatch",
                reason = "preflight_invalid",
                error = %err,
                "dispatch skipped due to invalid effective config"
            );
            self.emit_runtime_event(RuntimeEvent::ValidationSkippedDispatch);
            return Ok(TickResult {
                dispatched_issue_ids: vec![],
                dispatched_issues: vec![],
                dispatch_skipped: true,
            });
        }

        if let Err(err) = port.validate_dispatch_preflight(&self.config) {
            tracing::warn!(
                phase = "dispatch",
                reason = "preflight_invalid",
                error = %err,
                "dispatch skipped due to preflight validation failure"
            );
            self.emit_runtime_event(RuntimeEvent::ValidationSkippedDispatch);
            return Ok(TickResult {
                dispatched_issue_ids: vec![],
                dispatched_issues: vec![],
                dispatch_skipped: true,
            });
        }

        self.emit_runtime_event(RuntimeEvent::Dispatch);
        tracing::info!(phase = "dispatch", "starting orchestrator tick phase");

        let candidates = port.fetch_candidate_issues()?;
        let sorted_candidates = self.sort_issues_for_dispatch(candidates);
        let candidate_ids: std::collections::HashSet<String> =
            sorted_candidates.iter().map(|i| i.id.clone()).collect();
        let mut dispatched_issue_ids = vec![];
        let mut dispatched_issues = vec![];
        let mut blocked_entries: Vec<crate::domain::BlockedIssueEntry> = vec![];

        // First pass: identify all dependency-blocked candidates so the blocked list
        // is complete regardless of slot availability. Uses is_candidate_for_blocked_check
        // instead of should_dispatch_issue because should_dispatch_issue rejects on
        // slot exhaustion — blocked issues need to show even when all slots are full.
        for candidate in &sorted_candidates {
            if !self.is_candidate_for_blocked_check(candidate) {
                continue;
            }
            let (dep_blocked, blocker_ids) =
                self.is_blocked_by_dependency(candidate, &sorted_candidates, &candidate_ids);
            if dep_blocked {
                blocked_entries.push(crate::domain::BlockedIssueEntry {
                    issue_id: candidate.id.clone(),
                    identifier: candidate.identifier.clone(),
                    title: candidate.title.clone(),
                    state: candidate.state.clone(),
                    blocker_identifiers: blocker_ids,
                });
            }
        }

        // Second pass: dispatch non-blocked candidates until slots are exhausted.
        let blocked_ids: std::collections::HashSet<&str> = blocked_entries
            .iter()
            .map(|e| e.issue_id.as_str())
            .collect();

        for candidate in &sorted_candidates {
            if self.available_slots() == 0 {
                tracing::debug!(
                    phase = "dispatch",
                    reason = "slot_full",
                    "global concurrency slots exhausted"
                );
                break;
            }

            if !self.should_dispatch_issue(candidate) {
                tracing::debug!(
                    phase = "dispatch",
                    reason = "blocked",
                    issue_id = %candidate.id,
                    issue_identifier = %candidate.identifier,
                    "candidate rejected before refresh"
                );
                continue;
            }

            // Skip dependency-blocked candidates (already collected above)
            if blocked_ids.contains(candidate.id.as_str()) {
                continue;
            }

            let Some(refreshed_issue) = port.refresh_issue(&candidate.id)? else {
                tracing::debug!(
                    phase = "dispatch",
                    reason = "blocked",
                    issue_id = %candidate.id,
                    issue_identifier = %candidate.identifier,
                    "candidate missing at pre-dispatch refresh"
                );
                continue;
            };

            let workspace_refresh_policy =
                Self::workspace_refresh_policy_for_dispatch(&refreshed_issue, None);
            let Some(workspace_preparation) = self
                .prepare_workspace_for_active_dispatch(&refreshed_issue, workspace_refresh_policy)
            else {
                continue;
            };
            let workspace_path = workspace_preparation.path.clone();

            let Some(refreshed_issue) =
                self.enforce_agent_review_pr_gate(&refreshed_issue, &workspace_path, port)?
            else {
                continue;
            };

            if !self.should_dispatch_issue(&refreshed_issue) {
                tracing::debug!(
                    phase = "dispatch",
                    reason = "blocked",
                    issue_id = %refreshed_issue.id,
                    issue_identifier = %refreshed_issue.identifier,
                    "candidate rejected after pre-dispatch refresh"
                );
                continue;
            }

            // Select an SSH host (or local) for this fresh dispatch.
            let host_selection = self.select_worker_host(None);
            if matches!(host_selection, WorkerHostSelection::NoneAvailable) {
                tracing::warn!(
                    event = "ssh_pool_exhausted",
                    issue_id = %refreshed_issue.id,
                    issue_identifier = %refreshed_issue.identifier,
                    "SSH host pool exhausted, deferring dispatch"
                );
                continue;
            }
            let worker_host = match host_selection {
                WorkerHostSelection::Remote(ref host) => Some(host.clone()),
                _ => None,
            };
            self.dispatch_issue(
                &refreshed_issue,
                None,
                Some(workspace_path),
                worker_host.clone(),
            );
            dispatched_issue_ids.push(refreshed_issue.id.clone());
            dispatched_issues.push(DispatchedIssue {
                issue: refreshed_issue,
                attempt: None,
                worker_host,
                workspace_refresh_policy,
                workspace_status_context: workspace_preparation.status_context,
            });
        }

        // Store blocked issues for snapshot visibility
        self.blocked_issues = blocked_entries;

        Ok(TickResult {
            dispatched_issue_ids,
            dispatched_issues,
            dispatch_skipped: false,
        })
    }

    pub fn schedule_retry(
        &mut self,
        issue_id: &str,
        identifier: &str,
        attempt: u32,
        retry_kind: RetryKind,
        now_ms: i64,
        error: Option<String>,
    ) -> String {
        self.schedule_retry_with_context(
            issue_id,
            identifier,
            attempt,
            retry_kind,
            now_ms,
            error,
            RetryContext::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn schedule_retry_with_context(
        &mut self,
        issue_id: &str,
        identifier: &str,
        attempt: u32,
        retry_kind: RetryKind,
        now_ms: i64,
        error: Option<String>,
        context: RetryContext,
    ) -> String {
        self.next_retry_token += 1;
        let token = format!("retry-{}", self.next_retry_token);
        let due_at_ms = now_ms + self.retry_delay_ms(retry_kind, attempt);

        self.retry_tokens
            .insert(issue_id.to_string(), token.clone());

        self.state.retry_attempts.insert(
            issue_id.to_string(),
            RetryEntry {
                issue_id: issue_id.to_string(),
                identifier: identifier.to_string(),
                attempt,
                due_at_ms,
                timer_handle: Some(token.clone()),
                error: error.clone(),
                worker_host: context.worker_host.clone(),
                workspace_path: context.workspace_path.clone(),
            },
        );

        if let Some(session_id) = context.session_id.as_ref() {
            self.worker_session_ids
                .insert(issue_id.to_string(), session_id.clone());
        }

        tracing::info!(
            event = "retry_scheduled",
            issue_id = %issue_id,
            issue_identifier = %identifier,
            retry_kind = ?retry_kind,
            attempt,
            due_at_ms,
            token = %token,
            session_id = context.session_id.as_deref().unwrap_or("n/a"),
            worker_host = context.worker_host.as_deref().unwrap_or("local"),
            workspace_path = context.workspace_path.as_deref().unwrap_or("n/a"),
            error = error.as_deref().unwrap_or(""),
            "queued issue retry"
        );

        self.emit_runtime_event(RuntimeEvent::RetryScheduled {
            issue_id: issue_id.to_string(),
            attempt,
            due_at_ms,
            token: token.clone(),
            retry_kind,
        });

        token
    }

    pub fn fire_retry(&mut self, issue_id: &str, token: &str) -> bool {
        let Some(current_token) = self.retry_tokens.get(issue_id).cloned() else {
            return false;
        };

        if current_token != token {
            tracing::info!(
                event = "retry_ignored_stale",
                issue_id = %issue_id,
                token = %token,
                current_token = %current_token,
                "ignored stale retry timer firing"
            );

            self.emit_runtime_event(RuntimeEvent::RetryIgnoredStale {
                issue_id: issue_id.to_string(),
                token: token.to_string(),
            });
            return false;
        }

        self.retry_tokens.remove(issue_id);
        self.state.retry_attempts.remove(issue_id).is_some()
    }

    pub fn record_worker_activity(&mut self, issue_id: &str, timestamp_ms: i64) {
        self.worker_last_activity_ms
            .insert(issue_id.to_string(), timestamp_ms);
        if let Some(info) = self.worker_session_info.get_mut(issue_id) {
            info.last_activity_ms = Some(timestamp_ms);
        }
    }

    fn ensure_worker_session_info(&mut self, issue_id: &str) -> &mut WorkerSessionInfo {
        let max_turns = self.config.agent.max_turns.max(1);
        let stall_timeout_ms = backend_stall_timeout_ms(&self.config, self.config.agent_backend);
        let last_activity_ms = self.worker_last_activity_ms.get(issue_id).copied();
        let info = self
            .worker_session_info
            .entry(issue_id.to_string())
            .or_insert(WorkerSessionInfo {
                turn_count: 1,
                max_turns,
                stall_timeout_ms,
                last_activity_ms,
                session_tokens: SessionTokenUsage::default(),
                current_tool_name: None,
                current_tool_args_preview: None,
                last_error: None,
            });
        if info.max_turns == 0 {
            info.max_turns = max_turns;
        }
        if info.stall_timeout_ms <= 0 {
            info.stall_timeout_ms = stall_timeout_ms;
        }
        if info.turn_count == 0 {
            info.turn_count = 1;
        }
        if info.last_activity_ms.is_none() {
            info.last_activity_ms = last_activity_ms;
        }
        info
    }

    fn advance_turn_counter(&mut self, issue_id: &str) {
        let session_info = self.ensure_worker_session_info(issue_id);
        let max_turns = session_info.max_turns.max(1);
        let current = session_info.turn_count.max(1);
        session_info.turn_count = current.saturating_add(1).min(max_turns);
    }

    pub fn ingest_agent_event(&mut self, issue_id: &str, event: &AgentEvent) {
        if let Some(request_id) = match event {
            AgentEvent::EscalationResponded { request_id, .. }
            | AgentEvent::EscalationTimedOut { request_id, .. }
            | AgentEvent::EscalationCancelled { request_id, .. } => Some(request_id.as_str()),
            _ => None,
        } {
            let _ = self.escalation_registry.remove(request_id);
        }

        if !self.state.running.contains_key(issue_id) {
            tracing::debug!(
                issue_id = %issue_id,
                event = %event_name(event),
                "ignored codex worker event for non-running issue"
            );
            return;
        }

        self.state.codex_totals.event_count = self.state.codex_totals.event_count.saturating_add(1);

        let _ = self.ensure_worker_session_info(issue_id);
        self.record_worker_activity(issue_id, event_timestamp_ms(event));
        if let Some(session_id) = event_session_id(event) {
            self.worker_session_ids
                .insert(issue_id.to_string(), session_id.to_string());
        }
        self.publish_agent_event_to_hub(issue_id, event);
        let event_time = event_timestamp(event);

        let (last_event, last_event_message) = event_summary(event);
        let session_stats = self
            .running_session_stats
            .entry(issue_id.to_string())
            .or_default();
        session_stats.last_activity_at = Some(event_time);
        session_stats.last_event = Some(last_event);
        session_stats.last_event_message = last_event_message;
        session_stats.session_id = self.worker_session_ids.get(issue_id).cloned();

        // Track current tool activity from events.
        let tool_activity = extract_tool_activity(event);
        match tool_activity {
            ToolActivity::Started { name, args_preview } => {
                session_stats.current_tool_name = Some(name.clone());
                session_stats.current_tool_args_preview = args_preview.clone();
                if let Some(info) = self.worker_session_info.get_mut(issue_id) {
                    info.current_tool_name = Some(name);
                    info.current_tool_args_preview = args_preview;
                }
            }
            ToolActivity::Ended => {
                session_stats.current_tool_name = None;
                session_stats.current_tool_args_preview = None;
                if let Some(info) = self.worker_session_info.get_mut(issue_id) {
                    info.current_tool_name = None;
                    info.current_tool_args_preview = None;
                }
            }
            ToolActivity::None => {}
        }

        if let Some(run_attempt) = self.state.running.get_mut(issue_id) {
            match event {
                AgentEvent::TurnFailed { error, .. } | AgentEvent::StartupFailed { error, .. } => {
                    run_attempt.status = "failed".to_string();
                    run_attempt.error = Some(error.clone());
                }
                AgentEvent::TurnCancelled { .. } => {
                    run_attempt.status = "cancelled".to_string();
                }
                // TurnEndedWithError is non-fatal: pi-agent retries internally.
                // Don't mark the run as failed — just surface the error transiently.
                _ => {}
            }
        }

        let event_error = match event {
            AgentEvent::TurnFailed { error, .. } | AgentEvent::StartupFailed { error, .. } => {
                Some(error.as_str())
            }
            _ => None,
        };

        if let Some(error) = event_error {
            if let Some(info) = self.worker_session_info.get_mut(issue_id) {
                info.last_error = Some(format_rate_limit_error(error));
            }
        }

        // TurnEndedWithError: surface the error transiently in the TUI
        // but don't mark the run as failed — pi-agent retries internally.
        if let AgentEvent::TurnEndedWithError { error, .. } = event {
            if let Some(info) = self.worker_session_info.get_mut(issue_id) {
                info.last_error = Some(format_rate_limit_error(error));
            }
        }

        match event {
            AgentEvent::EscalationCreated { request, .. } => {
                tracing::info!(
                    event = "escalation_created",
                    issue_id = %request.issue_id,
                    request_id = %request.id,
                    method = %request.method,
                    "worker escalation created"
                );
            }
            AgentEvent::EscalationResponded {
                request_id,
                responder_id,
                latency_ms,
                ..
            } => {
                tracing::info!(
                    event = "escalation_responded",
                    request_id = %request_id,
                    responder = %responder_id.as_deref().unwrap_or("operator"),
                    latency_ms,
                    "worker escalation answered"
                );
            }
            AgentEvent::EscalationTimedOut {
                request_id,
                timeout_ms,
                ..
            } => {
                tracing::warn!(
                    event = "escalation_timed_out",
                    request_id = %request_id,
                    timeout_ms,
                    "worker escalation timed out"
                );
            }
            AgentEvent::EscalationCancelled {
                request_id, reason, ..
            } => {
                tracing::warn!(
                    event = "escalation_cancelled",
                    request_id = %request_id,
                    reason = %reason,
                    "worker escalation cancelled"
                );
            }
            _ => {}
        }

        if let AgentEvent::TurnCompleted {
            input_tokens,
            output_tokens,
            total_tokens,
            rate_limits,
            ..
        } = event
        {
            session_stats.turn_count = session_stats.turn_count.saturating_add(1);
            session_stats.total_tokens = session_stats.total_tokens.saturating_add(*total_tokens);
            let session_info = self
                .worker_session_info
                .get_mut(issue_id)
                .expect("session info must exist after ensure_worker_session_info");
            session_info.session_tokens.input_tokens = session_info
                .session_tokens
                .input_tokens
                .saturating_add(*input_tokens);
            session_info.session_tokens.output_tokens = session_info
                .session_tokens
                .output_tokens
                .saturating_add(*output_tokens);
            session_info.session_tokens.total_tokens = session_info
                .session_tokens
                .total_tokens
                .saturating_add(*total_tokens);
            session_info.last_error = None;
            self.advance_turn_counter(issue_id);
            self.apply_turn_metrics(&TurnMetrics {
                input_tokens: *input_tokens,
                output_tokens: *output_tokens,
                total_tokens: *total_tokens,
                rate_limits: rate_limits.clone(),
            });
        }

        tracing::debug!(
            issue_id = %issue_id,
            session_id = self
                .worker_session_ids
                .get(issue_id)
                .map(String::as_str)
                .unwrap_or("n/a"),
            event = %event_name(event),
            "ingested codex worker event"
        );
    }

    fn cancel_pending_escalations_for_issue(&mut self, issue_id: &str, reason: &str) {
        let cancelled_requests = self.escalation_registry.cancel_for_issue(issue_id);
        if cancelled_requests.is_empty() {
            return;
        }

        let issue_is_running = self.state.running.contains_key(issue_id);

        for request in cancelled_requests {
            if issue_is_running {
                let event = AgentEvent::EscalationCancelled {
                    timestamp: Utc::now(),
                    issue_id: request.issue_id.clone(),
                    issue_identifier: request.issue_identifier.clone(),
                    request_id: request.id.clone(),
                    reason: reason.to_string(),
                };
                self.ingest_agent_event(issue_id, &event);
            } else {
                tracing::warn!(
                    event = "escalation_cancelled",
                    issue_id = %request.issue_id,
                    request_id = %request.id,
                    reason = %reason,
                    "cleaned pending escalation for released worker"
                );
            }
        }
    }

    pub fn handle_worker_completion(
        &mut self,
        issue_id: &str,
        completion: WorkerCompletion,
        now_ms: i64,
    ) -> Option<String> {
        let Some(_existing_attempt) = self.state.running.get(issue_id).cloned() else {
            self.cancel_pending_escalations_for_issue(issue_id, "worker_already_released");

            if self.config.workspace.cleanup_on_done {
                if let Some(pending) = self.pending_terminal_cleanup.remove(issue_id) {
                    self.cleanup_workspace(&pending.issue, &pending.workspace_path);
                }
            } else {
                self.pending_terminal_cleanup.remove(issue_id);
            }
            return None;
        };

        self.cancel_pending_escalations_for_issue(issue_id, "worker_exited");

        let run_attempt = self.state.running.remove(issue_id)?;
        self.state.claimed.remove(issue_id);
        // Keep running_issue_states until reconciliation — needed for
        // state-change notification detection on the next poll cycle.
        self.worker_last_activity_ms.remove(issue_id);

        let completed_turn_count = self
            .running_session_stats
            .get(issue_id)
            .map(|stats| stats.turn_count)
            .or_else(|| {
                self.worker_session_info
                    .get(issue_id)
                    .map(|info| info.turn_count.saturating_sub(1))
            })
            .unwrap_or_default();
        let completed_total_tokens = self
            .running_session_stats
            .get(issue_id)
            .map(|stats| stats.total_tokens)
            .or_else(|| {
                self.worker_session_info
                    .get(issue_id)
                    .map(|info| info.session_tokens.total_tokens)
            })
            .unwrap_or_default();

        self.completion_comment_summaries.insert(
            issue_id.to_string(),
            CompletionCommentSummary {
                turn_count: completed_turn_count,
                total_tokens: completed_total_tokens,
                duration: Utc::now().signed_duration_since(run_attempt.started_at),
                worker_host: run_attempt.worker_host.clone(),
            },
        );

        self.running_session_stats.remove(issue_id);
        self.worker_session_info.remove(issue_id);
        self.running_parent_identifiers.remove(issue_id);

        let issue_identifier = run_attempt.issue_identifier.clone();
        let session_id = self.worker_session_ids.remove(issue_id);
        self.worker_steer_tx.remove(issue_id);
        let retry_context = RetryContext {
            worker_host: run_attempt.worker_host.clone(),
            workspace_path: Some(run_attempt.workspace_path.clone()),
            session_id: session_id.clone(),
        };

        match completion {
            WorkerCompletion::Completed {
                schedule_continuation,
            } => {
                if !schedule_continuation {
                    self.state.completed.insert(
                        issue_id.to_string(),
                        CompletedEntry {
                            issue_id: issue_id.to_string(),
                            identifier: issue_identifier.clone(),
                            title: run_attempt.issue_title.clone().unwrap_or_default(),
                            completed_at: Some(Utc::now()),
                        },
                    );
                }

                tracing::info!(
                    event = "worker_completed",
                    issue_id = %issue_id,
                    issue_identifier = %issue_identifier,
                    session_id = session_id.as_deref().unwrap_or("n/a"),
                    schedule_continuation,
                    "worker attempt completed"
                );

                self.emit_runtime_event(RuntimeEvent::WorkerCompleted {
                    issue_id: issue_id.to_string(),
                    issue_identifier: issue_identifier.clone(),
                    session_id,
                });

                if schedule_continuation {
                    Some(self.schedule_retry_with_context(
                        issue_id,
                        &issue_identifier,
                        1,
                        RetryKind::Continuation,
                        now_ms,
                        None,
                        retry_context,
                    ))
                } else {
                    self.state.retry_attempts.remove(issue_id);
                    None
                }
            }
            WorkerCompletion::Failed { error } => {
                self.state.completed.remove(issue_id);

                let attempt = run_attempt.attempt.unwrap_or(0).saturating_add(1).max(1);

                tracing::warn!(
                    event = "worker_failed",
                    issue_id = %issue_id,
                    issue_identifier = %issue_identifier,
                    session_id = session_id.as_deref().unwrap_or("n/a"),
                    attempt,
                    error = %error,
                    "worker attempt failed; scheduling failure retry"
                );

                self.emit_runtime_event(RuntimeEvent::WorkerFailed {
                    issue_id: issue_id.to_string(),
                    issue_identifier: issue_identifier.clone(),
                    session_id,
                    error: error.clone(),
                });

                let issue_title = run_attempt
                    .issue_title
                    .clone()
                    .unwrap_or_else(|| issue_identifier.clone());
                let is_stall_failure = error.contains(STALL_FAILURE_MARKER);
                if !is_stall_failure {
                    self.queue_slack_notification(
                        "failed",
                        &issue_identifier,
                        &issue_title,
                        "Agent failed during execution.",
                        run_attempt.issue_url.as_deref(),
                    );
                }

                Some(self.schedule_retry_with_context(
                    issue_id,
                    &issue_identifier,
                    attempt,
                    RetryKind::Failure,
                    now_ms,
                    Some(error),
                    retry_context,
                ))
            }
        }
    }

    pub async fn execute_worker_attempt<E, EFut>(
        &mut self,
        issue: &Issue,
        prompt_template: &str,
        attempt: Option<u32>,
        graphql_executor: E,
    ) -> Result<()>
    where
        E: Fn(String, serde_json::Value) -> EFut + Clone + Send,
        EFut: Future<Output = Result<serde_json::Value>> + Send,
    {
        let hook_cwd = workflow_dir_from_path(&self.workflow_path);
        let workspace_info = workspace::ensure_workspace_for_issue_with_hook_cwd(
            issue,
            &self.config.workspace,
            &self.config.hooks,
            &hook_cwd,
        )?;

        // Preserve the worker_host that dispatch_issue() already stored on the
        // scheduled RunAttempt (if present) so SSH dispatch is honoured here.
        let prior_worker_host = self
            .state
            .running
            .get(&issue.id)
            .and_then(|a| a.worker_host.clone());
        let pi_model_override = if self.config.agent_backend == AgentBackend::KataCli {
            effective_pi_model_for_issue(&self.config, issue)
        } else {
            None
        };

        self.completion_comment_summaries.remove(&issue.id);
        self.state.running.insert(
            issue.id.clone(),
            RunAttempt {
                issue_id: issue.id.clone(),
                issue_identifier: issue.identifier.clone(),
                issue_title: Some(issue.title.clone()),
                attempt,
                workspace_path: workspace_info.path.clone(),
                started_at: Utc::now(),
                status: "running".to_string(),
                error: None,
                worker_host: prior_worker_host.clone(),
                model: pi_model_override.clone(),
                tracker_state: Some(issue.state.clone()),
                issue_url: issue.url.clone(),
            },
        );
        let _ = self.ensure_worker_session_info(&issue.id);
        self.state.claimed.insert(issue.id.clone());
        self.running_issue_states
            .insert(issue.id.clone(), issue.state.clone());
        self.running_parent_identifiers
            .insert(issue.id.clone(), issue.parent_identifier.clone());
        self.running_session_stats
            .entry(issue.id.clone())
            .or_insert_with(|| RunningSessionStats {
                turn_count: 0,
                last_activity_at: Some(Utc::now()),
                total_tokens: 0,
                last_event: None,
                last_event_message: None,
                session_id: None,
                current_tool_name: None,
                current_tool_args_preview: None,
            });
        self.state.retry_attempts.remove(&issue.id);

        let workspace_path = Path::new(&workspace_info.path);

        if let Err(err) = workspace::run_before_run_hook_for_issue_with_cwd(
            workspace_path,
            &self.config.hooks,
            issue,
            &hook_cwd,
        ) {
            self.handle_worker_completion(
                &issue.id,
                WorkerCompletion::Failed {
                    error: err.to_string(),
                },
                Utc::now().timestamp_millis(),
            );
            return Err(err);
        }

        let prompt_template = Self::ensure_shared_context_placeholder(prompt_template.to_string());
        let shared_context = self.build_shared_context_block_for_issue(issue);

        let prompt = match prompt_builder::render_prompt_with_shared_context(
            &prompt_template,
            issue,
            attempt,
            self.config.workspace.base_branch.as_deref(),
            &shared_context,
        ) {
            Ok(prompt) => prompt,
            Err(err) => {
                self.handle_worker_completion(
                    &issue.id,
                    WorkerCompletion::Failed {
                        error: err.to_string(),
                    },
                    Utc::now().timestamp_millis(),
                );
                return Err(err);
            }
        };

        let loop_result = match self.config.agent_backend {
            AgentBackend::Codex => {
                let symphony_bin = std::env::current_exe()
                    .ok()
                    .map(|path| path.to_string_lossy().to_string());
                let symphony_workflow_path = Some(self.workflow_path.to_string_lossy().to_string());
                let mut session = match app_server::start_session_with_helper_env(
                    &self.config.codex,
                    issue,
                    workspace_path,
                    Path::new(&self.config.workspace.root),
                    prior_worker_host.as_deref(),
                    None,
                    app_server::HelperEnv {
                        symphony_bin: symphony_bin.as_deref(),
                        symphony_workflow_path: symphony_workflow_path.as_deref(),
                    },
                )
                .await
                {
                    Ok(session) => session,
                    Err(err) => {
                        self.handle_worker_completion(
                            &issue.id,
                            WorkerCompletion::Failed {
                                error: err.to_string(),
                            },
                            Utc::now().timestamp_millis(),
                        );
                        return Err(err);
                    }
                };

                tracing::info!(
                    event = "worker_started",
                    backend = "codex",
                    issue_id = %issue.id,
                    issue_identifier = %issue.identifier,
                    session_id = %session.session_id,
                    workspace_path = %workspace_info.path,
                    "worker attempt started"
                );

                let loop_result = run_codex_turns_in_session(
                    &mut session,
                    issue,
                    prompt.clone(),
                    self.config.agent.max_turns,
                    &self.config.tracker,
                    graphql_executor.clone(),
                    |_event| {},
                )
                .await;

                if let Err(err) = app_server::stop_session(session).await {
                    tracing::warn!(
                        issue_id = %issue.id,
                        issue_identifier = %issue.identifier,
                        error = %err,
                        "failed to stop codex session cleanly"
                    );
                }

                loop_result
            }
            AgentBackend::KataCli => {
                let mut session = match rpc_bridge::start_session(
                    &self.config.pi_agent,
                    issue,
                    workspace_path,
                    Path::new(&self.config.workspace.root),
                    rpc_bridge::StartSessionOptions {
                        worker_host: prior_worker_host.clone(),
                        container_id: None,
                        escalation_tx: self.worker_escalation_tx.clone(),
                        escalation_timeout_ms: self.config.agent.escalation_timeout_ms,
                        model_override: pi_model_override.clone(),
                        symphony_bin: std::env::current_exe()
                            .ok()
                            .map(|path| path.to_string_lossy().to_string()),
                        symphony_workflow_path: Some(
                            self.workflow_path.to_string_lossy().to_string(),
                        ),
                    },
                )
                .await
                {
                    Ok(session) => session,
                    Err(err) => {
                        self.handle_worker_completion(
                            &issue.id,
                            WorkerCompletion::Failed {
                                error: err.to_string(),
                            },
                            Utc::now().timestamp_millis(),
                        );
                        return Err(err);
                    }
                };

                tracing::info!(
                    event = "worker_started",
                    backend = "pi",
                    issue_id = %issue.id,
                    issue_identifier = %issue.identifier,
                    session_id = %session.session_id,
                    workspace_path = %workspace_info.path,
                    "worker attempt started"
                );

                let mut steer_rx = None;
                let loop_result = run_pi_turns_in_session(
                    &mut session,
                    issue,
                    prompt,
                    self.config.agent.max_turns,
                    &self.config.tracker,
                    &mut steer_rx,
                    |_event| {},
                )
                .await;

                if let Err(err) = rpc_bridge::stop_session(session).await {
                    tracing::warn!(
                        issue_id = %issue.id,
                        issue_identifier = %issue.identifier,
                        error = %err,
                        "failed to stop pi session cleanly"
                    );
                }

                loop_result
            }
        };

        let observed_events = match &loop_result {
            Ok(success) => &success.events,
            Err(failure) => &failure.events,
        };

        for event in observed_events {
            self.ingest_agent_event(&issue.id, event);
        }

        let _ = workspace::run_after_run_hook_for_issue_with_cwd(
            workspace_path,
            &self.config.hooks,
            issue,
            &hook_cwd,
        );

        match loop_result {
            Ok(success) => {
                self.handle_worker_completion(
                    &issue.id,
                    WorkerCompletion::Completed {
                        schedule_continuation: success.schedule_continuation,
                    },
                    Utc::now().timestamp_millis(),
                );

                Ok(())
            }
            Err(failure) => {
                let error = failure.error;
                let error_text = error.to_string();
                self.handle_worker_completion(
                    &issue.id,
                    WorkerCompletion::Failed { error: error_text },
                    Utc::now().timestamp_millis(),
                );
                Err(error)
            }
        }
    }

    pub fn detect_stalled_workers(&mut self, now_ms: i64, stall_timeout_ms: i64) {
        let running_issue_ids: Vec<String> = self.state.running.keys().cloned().collect();

        for issue_id in running_issue_ids {
            let Some(run_attempt) = self.state.running.get(&issue_id).cloned() else {
                continue;
            };
            let per_session_stall_timeout_ms = self
                .worker_session_info
                .get(&issue_id)
                .map(|info| info.stall_timeout_ms)
                .filter(|timeout| *timeout > 0)
                .unwrap_or(stall_timeout_ms);
            if per_session_stall_timeout_ms <= 0 {
                continue;
            }

            let last_activity_ms = self
                .worker_last_activity_ms
                .get(&issue_id)
                .copied()
                .unwrap_or_else(|| run_attempt.started_at.timestamp_millis());

            let elapsed_ms = now_ms.saturating_sub(last_activity_ms);
            if elapsed_ms <= per_session_stall_timeout_ms {
                continue;
            }

            let session_id = self.worker_session_ids.get(&issue_id).cloned();

            tracing::warn!(
                event = "worker_stalled",
                issue_id = %issue_id,
                issue_identifier = %run_attempt.issue_identifier,
                session_id = session_id.as_deref().unwrap_or("n/a"),
                elapsed_ms,
                stall_timeout_ms = per_session_stall_timeout_ms,
                "detected stalled worker; scheduling failure retry"
            );

            self.emit_runtime_event(RuntimeEvent::WorkerStalled {
                issue_id: issue_id.clone(),
                issue_identifier: run_attempt.issue_identifier.clone(),
                session_id,
                elapsed_ms,
            });

            let issue_title = run_attempt
                .issue_title
                .clone()
                .unwrap_or_else(|| run_attempt.issue_identifier.clone());
            self.queue_slack_notification(
                "stalled",
                &run_attempt.issue_identifier,
                &issue_title,
                &format!("No activity for {} seconds.", elapsed_ms / 1000),
                run_attempt.issue_url.as_deref(),
            );

            self.handle_worker_completion(
                &issue_id,
                WorkerCompletion::Failed {
                    error: format!("stalled for {elapsed_ms}ms {STALL_FAILURE_MARKER}"),
                },
                now_ms,
            );
        }
    }

    pub fn apply_turn_metrics(&mut self, metrics: &TurnMetrics) {
        self.state.codex_totals.input_tokens = self
            .state
            .codex_totals
            .input_tokens
            .saturating_add(metrics.input_tokens);
        self.state.codex_totals.output_tokens = self
            .state
            .codex_totals
            .output_tokens
            .saturating_add(metrics.output_tokens);
        self.state.codex_totals.total_tokens = self
            .state
            .codex_totals
            .total_tokens
            .saturating_add(metrics.total_tokens);

        if let Some(rate_limits) = metrics.rate_limits.clone() {
            self.state.codex_rate_limits = Some(rate_limit_info(rate_limits));
        }

        tracing::info!(
            event = "token_aggregate_updated",
            input_delta = metrics.input_tokens,
            output_delta = metrics.output_tokens,
            total_delta = metrics.total_tokens,
            input_total = self.state.codex_totals.input_tokens,
            output_total = self.state.codex_totals.output_tokens,
            total_total = self.state.codex_totals.total_tokens,
            has_rate_limits = metrics.rate_limits.is_some(),
            "updated codex aggregate token totals"
        );
    }

    fn emit_runtime_event(&mut self, event: RuntimeEvent) {
        self.publish_runtime_event_to_hub(&event);
        self.events.push(event);
    }

    fn publish_runtime_event_to_hub(&self, event: &RuntimeEvent) {
        let Some(hub) = &self.event_hub else {
            return;
        };

        let (kind, severity, issue, event_name, payload) = match event {
            RuntimeEvent::StartupCleanup => (
                EventKind::Runtime,
                EventSeverity::Info,
                None,
                "startup_cleanup",
                serde_json::json!({}),
            ),
            RuntimeEvent::Reconcile => (
                EventKind::Runtime,
                EventSeverity::Debug,
                None,
                "reconcile",
                serde_json::json!({}),
            ),
            RuntimeEvent::Validate => (
                EventKind::Runtime,
                EventSeverity::Debug,
                None,
                "validate",
                serde_json::json!({}),
            ),
            RuntimeEvent::Dispatch => (
                EventKind::Runtime,
                EventSeverity::Info,
                None,
                "dispatch",
                serde_json::json!({}),
            ),
            RuntimeEvent::ValidationSkippedDispatch => (
                EventKind::Runtime,
                EventSeverity::Warn,
                None,
                "validation_skipped_dispatch",
                serde_json::json!({}),
            ),
            RuntimeEvent::RetryScheduled {
                issue_id,
                attempt,
                due_at_ms,
                token,
                retry_kind,
            } => (
                EventKind::Runtime,
                EventSeverity::Info,
                self.issue_identifier_from_issue_id(issue_id),
                "retry_scheduled",
                serde_json::json!({
                    "issue_id": issue_id,
                    "attempt": attempt,
                    "due_at_ms": due_at_ms,
                    "token": token,
                    "retry_kind": format!("{:?}", retry_kind).to_ascii_lowercase(),
                }),
            ),
            RuntimeEvent::RetryIgnoredStale { issue_id, token } => (
                EventKind::Runtime,
                EventSeverity::Debug,
                self.issue_identifier_from_issue_id(issue_id),
                "retry_ignored_stale",
                serde_json::json!({
                    "issue_id": issue_id,
                    "token": token,
                }),
            ),
            RuntimeEvent::WorkerCompleted {
                issue_id,
                issue_identifier,
                session_id,
            } => (
                EventKind::Worker,
                EventSeverity::Info,
                Some(issue_identifier.clone()),
                "worker_completed",
                serde_json::json!({
                    "issue_id": issue_id,
                    "session_id": session_id,
                }),
            ),
            RuntimeEvent::WorkerFailed {
                issue_id,
                issue_identifier,
                session_id,
                error,
            } => (
                EventKind::Worker,
                EventSeverity::Error,
                Some(issue_identifier.clone()),
                "worker_failed",
                serde_json::json!({
                    "issue_id": issue_id,
                    "session_id": session_id,
                    "error": truncate_for_display(error, 160),
                }),
            ),
            RuntimeEvent::WorkspacePrepareFailed {
                issue_id,
                issue_identifier,
                error,
            } => (
                EventKind::Runtime,
                EventSeverity::Error,
                Some(issue_identifier.clone()),
                "workspace_prepare_failed",
                serde_json::json!({
                    "issue_id": issue_id,
                    "error": truncate_for_display(error, 240),
                }),
            ),
            RuntimeEvent::WorkerStalled {
                issue_id,
                issue_identifier,
                session_id,
                elapsed_ms,
            } => (
                EventKind::Worker,
                EventSeverity::Warn,
                Some(issue_identifier.clone()),
                "worker_stalled",
                serde_json::json!({
                    "issue_id": issue_id,
                    "session_id": session_id,
                    "elapsed_ms": elapsed_ms,
                }),
            ),
            RuntimeEvent::SteerReceived {
                issue_identifier,
                instruction_preview,
            } => (
                EventKind::Runtime,
                EventSeverity::Info,
                Some(issue_identifier.clone()),
                "steer_received",
                serde_json::json!({
                    "issue_identifier": issue_identifier,
                    "instruction_preview": instruction_preview,
                }),
            ),
            RuntimeEvent::SteerDelivered {
                issue_id,
                issue_identifier,
            } => (
                EventKind::Runtime,
                EventSeverity::Info,
                Some(issue_identifier.clone()),
                "steer_delivered",
                serde_json::json!({
                    "issue_id": issue_id,
                    "issue_identifier": issue_identifier,
                }),
            ),
            RuntimeEvent::SteerFailed {
                issue_identifier,
                error,
            } => (
                EventKind::Runtime,
                EventSeverity::Warn,
                Some(issue_identifier.clone()),
                "steer_failed",
                serde_json::json!({
                    "issue_identifier": issue_identifier,
                    "error": error,
                }),
            ),
            RuntimeEvent::RefreshRequested => (
                EventKind::Runtime,
                EventSeverity::Info,
                None,
                "refresh_requested",
                serde_json::json!({}),
            ),
            RuntimeEvent::RefreshCoalesced => (
                EventKind::Runtime,
                EventSeverity::Debug,
                None,
                "refresh_coalesced",
                serde_json::json!({}),
            ),
        };

        hub.publish(kind, severity, issue, event_name, payload);
    }

    fn publish_agent_event_to_hub(&self, issue_id: &str, event: &AgentEvent) {
        let Some(hub) = &self.event_hub else {
            return;
        };

        let (event_name, summary) = event_summary(event);
        let issue_identifier = self.issue_identifier_from_issue_id(issue_id);
        let kind = event_kind_for_agent_event(event);
        let severity = event_severity_for_agent_event(event);

        let mut payload = match event {
            AgentEvent::EscalationCreated { request, .. } => serde_json::json!({
                "request_id": request.id.clone(),
                "issue_id": request.issue_id.clone(),
                "issue_identifier": request.issue_identifier.clone(),
                "method": request.method.clone(),
                "payload": request.payload.clone(),
                "created_at": request.created_at,
                "timeout_ms": request.timeout_ms,
                "summary": summary,
            }),
            AgentEvent::EscalationResponded {
                request_id,
                responder_id,
                latency_ms,
                ..
            } => serde_json::json!({
                "request_id": request_id.clone(),
                "responder_id": responder_id.clone(),
                "latency_ms": latency_ms,
                "summary": summary,
            }),
            AgentEvent::EscalationTimedOut {
                request_id,
                timeout_ms,
                ..
            } => serde_json::json!({
                "request_id": request_id.clone(),
                "timeout_ms": timeout_ms,
                "summary": summary,
            }),
            AgentEvent::EscalationCancelled {
                request_id, reason, ..
            } => serde_json::json!({
                "request_id": request_id.clone(),
                "reason": reason.clone(),
                "summary": summary,
            }),
            _ => serde_json::json!({
                "summary": summary,
                "session_id": self.worker_session_ids.get(issue_id).cloned(),
            }),
        };
        if matches!(severity, EventSeverity::Error) {
            if let Some(error_preview) = self.error_preview_for_agent_event(issue_id, event) {
                if let Some(object) = payload.as_object_mut() {
                    object.insert(
                        "error_preview".to_string(),
                        serde_json::Value::String(error_preview),
                    );
                }
            }
        }

        hub.publish_with_timestamp(
            kind,
            severity,
            issue_identifier,
            event_name,
            payload,
            event_timestamp(event),
        );
    }

    fn error_preview_for_agent_event(&self, issue_id: &str, event: &AgentEvent) -> Option<String> {
        match event {
            AgentEvent::Notification { message, .. } => {
                let notification = parse_tool_notification(message)?;
                if notification.event_name != "tool_error" {
                    return None;
                }

                let tool_name = notification.tool_name.as_deref().unwrap_or("tool");
                let last_tool = self
                    .running_session_stats
                    .get(issue_id)
                    .and_then(|stats| stats.current_tool_name.as_deref());
                let args_preview = self
                    .running_session_stats
                    .get(issue_id)
                    .and_then(|stats| stats.current_tool_args_preview.as_deref())
                    .or(notification.args_preview.as_deref());

                match args_preview {
                    Some(preview)
                        if last_tool.is_none()
                            || last_tool == Some(tool_name)
                            || notification.tool_name.is_none() =>
                    {
                        Some(format!("{tool_name}: {preview}"))
                    }
                    _ => notification.summary.clone(),
                }
            }
            AgentEvent::ToolCallFailed { tool_name, .. } => tool_name
                .as_deref()
                .map(|tool_name| format!("tool failed: {tool_name}")),
            AgentEvent::TurnFailed { error, .. }
            | AgentEvent::StartupFailed { error, .. }
            | AgentEvent::TurnEndedWithError { error, .. } => Some(error.clone()),
            AgentEvent::Malformed {
                parse_error,
                raw_text,
                ..
            } => Some(format!(
                "malformed event: {parse_error}; {}",
                normalize_whitespace(raw_text)
            )),
            _ => None,
        }
        .map(|preview| truncate_for_display(&preview, 160))
        .filter(|preview| !preview.is_empty())
    }

    fn issue_identifier_from_issue_id(&self, issue_id: &str) -> Option<String> {
        self.state
            .running
            .get(issue_id)
            .map(|attempt| attempt.issue_identifier.clone())
            .or_else(|| {
                self.state
                    .completed
                    .get(issue_id)
                    .map(|entry| entry.identifier.clone())
            })
            .or_else(|| {
                self.state
                    .retry_attempts
                    .get(issue_id)
                    .map(|entry| entry.identifier.clone())
            })
    }

    pub fn events(&self) -> &[RuntimeEvent] {
        &self.events
    }

    pub fn state(&self) -> &OrchestratorState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut OrchestratorState {
        &mut self.state
    }

    pub fn shared_context_store(&self) -> SharedContextStore {
        self.shared_context_store.clone()
    }

    fn supervisor_snapshot(&self) -> SupervisorSnapshot {
        if let Some(supervisor) = &self.supervisor_agent {
            return supervisor.snapshot();
        }

        if self.config.supervisor.enabled {
            SupervisorSnapshot::idle(self.config.supervisor.model.clone())
        } else {
            SupervisorSnapshot::disabled(self.config.supervisor.model.clone())
        }
    }

    pub fn supervisor_is_running(&self) -> bool {
        self.supervisor_agent
            .as_ref()
            .is_some_and(SupervisorAgent::is_running)
    }

    pub fn ensure_supervisor_running(&mut self) -> Result<()> {
        if !self.config.supervisor.enabled {
            return Ok(());
        }

        if self
            .supervisor_agent
            .as_ref()
            .is_some_and(SupervisorAgent::is_running)
        {
            return Ok(());
        }

        if let Some(mut supervisor) = self.supervisor_agent.take() {
            supervisor.abort();
        }

        let event_hub = self.create_event_hub();
        let deps = SupervisorDependencies::new(
            event_hub,
            self.shared_context_store.clone(),
            self.escalation_registry.clone(),
        );

        let mut supervisor = SupervisorAgent::new(self.config.supervisor.clone(), deps);
        supervisor.start()?;

        tracing::info!(
            event = "supervisor_started",
            cooldown_ms = self.config.supervisor.steer_cooldown_ms,
            "supervisor started from orchestrator lifecycle"
        );

        self.supervisor_agent = Some(supervisor);
        Ok(())
    }

    pub async fn shutdown_supervisor(&mut self) {
        if let Some(mut supervisor) = self.supervisor_agent.take() {
            supervisor.stop().await;
            tracing::info!(
                event = "supervisor_stopped",
                "supervisor stopped from orchestrator lifecycle"
            );
        }
    }

    async fn sync_supervisor_lifecycle(&mut self) {
        if self.config.supervisor.enabled {
            if let Err(err) = self.ensure_supervisor_running() {
                tracing::warn!(
                    event = "supervisor_start_failed",
                    error = %err,
                    "failed to start supervisor"
                );
            }
        } else {
            self.shutdown_supervisor().await;
        }
    }

    fn shared_context_scopes_for_issue(issue: &Issue) -> Vec<ContextScope> {
        let mut scopes = vec![ContextScope::Project];

        for label in &issue.labels {
            let normalized = label.trim().to_ascii_lowercase();
            if !normalized.is_empty() {
                scopes.push(ContextScope::Label(normalized));
            }
        }

        scopes.sort();
        scopes.dedup();
        scopes
    }

    fn ensure_shared_context_placeholder(prompt_template: String) -> String {
        if SHARED_CONTEXT_PLACEHOLDER_RE.is_match(&prompt_template) {
            return prompt_template;
        }

        format!("{{{{ shared_context }}}}\n\n{prompt_template}")
    }

    fn format_shared_context_age(now: DateTime<Utc>, created_at: DateTime<Utc>) -> String {
        let age_seconds = now.signed_duration_since(created_at).num_seconds().max(0) as u64;

        if age_seconds < 60 {
            return format!("{age_seconds}s ago");
        }

        let age_minutes = age_seconds / 60;
        if age_minutes < 60 {
            return format!("{age_minutes}m ago");
        }

        let age_hours = age_minutes / 60;
        if age_hours < 24 {
            return format!("{age_hours}h ago");
        }

        let age_days = age_hours / 24;
        format!("{age_days}d ago")
    }

    fn append_workspace_status_context(
        mut shared_context: String,
        workspace_status_context: Option<&str>,
    ) -> String {
        let Some(workspace_status_context) = workspace_status_context else {
            return shared_context;
        };
        if workspace_status_context.trim().is_empty() {
            return shared_context;
        }
        if !shared_context.trim().is_empty() {
            shared_context.push_str("\n\n");
        }
        shared_context.push_str(workspace_status_context);
        shared_context
    }

    fn build_shared_context_block_for_issue(&self, issue: &Issue) -> String {
        let scopes = Self::shared_context_scopes_for_issue(issue);
        let entries: Vec<_> = self
            .shared_context_store
            .read(&scopes)
            .into_iter()
            .filter(|entry| entry.author_issue != issue.identifier)
            .take(10)
            .collect();

        if let Some(hub) = &self.event_hub {
            hub.publish(
                EventKind::Runtime,
                EventSeverity::Debug,
                Some(issue.identifier.clone()),
                "shared_context_read",
                serde_json::json!({
                    "reader_issue": issue.identifier,
                    "entries_count": entries.len(),
                    "scopes": scopes.iter().map(ContextScope::as_scope_key).collect::<Vec<_>>(),
                }),
            );
        }

        if entries.is_empty() {
            return String::new();
        }

        let now = Utc::now();
        let mut lines = vec![
            "## Shared Context from Other Workers".to_string(),
            String::new(),
        ];

        for entry in entries {
            let age = Self::format_shared_context_age(now, entry.created_at);
            lines.push(format!(
                "- [{}] ({age}, {}): {}",
                entry.author_issue, entry.scope, entry.content
            ));
        }

        lines.join("\n")
    }

    fn shared_context_preview(content: &str) -> String {
        truncate_for_display(content, 120)
    }

    fn publish_shared_context_expired_event(&self, entry: &ContextEntry, now: DateTime<Utc>) {
        let Some(hub) = &self.event_hub else {
            return;
        };

        let age_ms = now
            .signed_duration_since(entry.created_at)
            .num_milliseconds()
            .max(0);

        hub.publish_with_timestamp(
            EventKind::SharedContextExpired,
            EventSeverity::Info,
            Some(entry.author_issue.clone()),
            "shared_context_expired",
            serde_json::json!({
                "entry_id": entry.id,
                "author_issue": entry.author_issue,
                "scope": entry.scope.as_scope_key(),
                "age_ms": age_ms,
                "preview": Self::shared_context_preview(&entry.content),
            }),
            now,
        );
    }

    fn prune_expired_shared_context(&mut self, now: DateTime<Utc>) -> usize {
        let expired_entries = self.shared_context_store.prune_expired_entries_at(now);
        for entry in &expired_entries {
            self.publish_shared_context_expired_event(entry, now);
        }
        expired_entries.len()
    }

    /// Create a shared snapshot handle for concurrent HTTP reads.
    ///
    /// The handle is pre-loaded with the current snapshot. The orchestrator
    /// retains an internal reference and publishes updates after every
    /// material state change. Returns a clone-cheap handle for HTTP use.
    pub fn create_snapshot_handle(&mut self) -> SnapshotHandle {
        let snapshot = self.snapshot(Utc::now().timestamp_millis());
        let handle = SnapshotHandle::new(snapshot);
        self.snapshot_handle = Some(handle.clone());
        handle
    }

    /// Create and attach an event hub for websocket publication.
    pub fn create_event_hub(&mut self) -> EventHub {
        if let Some(hub) = &self.event_hub {
            return hub.clone();
        }

        let hub = EventHub::default_hub();
        self.event_hub = Some(hub.clone());
        hub
    }

    /// Attach an existing event hub.
    pub fn attach_event_hub(&mut self, hub: EventHub) {
        self.event_hub = Some(hub);
    }

    pub fn attach_triage_runtime(&mut self, runtime: TriageRuntime) {
        self.triage_runtime = Some(runtime);
    }

    pub fn triage_runtime_mut(&mut self) -> Option<&mut TriageRuntime> {
        self.triage_runtime.as_mut()
    }

    pub fn take_triage_factory_query(
        &self,
    ) -> Option<std::sync::Arc<dyn crate::http_server::FactoryRunQuery>> {
        self.triage_runtime
            .as_ref()
            .map(|runtime| std::sync::Arc::new(runtime.store()) as _)
    }

    /// Create a refresh control channel.
    ///
    /// Returns the sender half (clone-cheap, for HTTP handlers). The
    /// orchestrator retains the receiver and checks it in its runtime loop.
    pub fn create_refresh_channel(&mut self) -> RefreshSender {
        let (sender, receiver) = refresh_channel();
        self.refresh_receiver = Some(receiver);
        sender
    }

    /// Create a steer control sender used by the HTTP API.
    pub fn create_steer_sender(&self) -> SteerSender {
        self.steer_sender.clone()
    }

    /// Publish the current snapshot to the shared handle (if created).
    ///
    /// Called after every material state change in the runtime loop.
    /// No-op if `create_snapshot_handle()` was never called.
    /// Resolve the prompt template for a given issue state, using per-state
    /// prompts if configured, otherwise falling back to the monolith prompt_template.
    fn resolve_prompt_for_state(&self, issue_state: &str) -> String {
        if let Some(prompts) = &self.config.prompts {
            let workflow_dir = workflow_dir_from_path(&self.workflow_path);

            match prompt_builder::resolve_per_state_prompt(prompts, issue_state, &workflow_dir) {
                Ok(Some(template)) => {
                    tracing::debug!(
                        issue_state = %issue_state,
                        "resolved per-state prompt template"
                    );
                    return template;
                }
                Ok(None) => {
                    tracing::debug!(
                        issue_state = %issue_state,
                        "no per-state prompt for state; using monolith template"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        issue_state = %issue_state,
                        error = %err,
                        "failed to resolve per-state prompt; falling back to monolith template"
                    );
                }
            }
        }
        self.prompt_template.clone()
    }

    pub fn publish_snapshot(&self) {
        if let Some(handle) = &self.snapshot_handle {
            let snapshot = self.snapshot(Utc::now().timestamp_millis());
            handle.publish(snapshot);
        }
    }

    pub fn snapshot(&self, now_ms: i64) -> OrchestratorSnapshot {
        let running: BTreeMap<String, RunAttempt> = self
            .state
            .running
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let running_sessions: BTreeMap<String, RunningSessionSnapshot> = self
            .state
            .running
            .iter()
            .map(|(issue_id, run_attempt)| {
                let stats = self.running_session_stats.get(issue_id);
                (
                    issue_id.clone(),
                    RunningSessionSnapshot {
                        turn_count: stats.map(|s| s.turn_count).unwrap_or(0),
                        last_activity_at: stats
                            .and_then(|s| s.last_activity_at)
                            .or(Some(run_attempt.started_at)),
                        total_tokens: stats.map(|s| s.total_tokens).unwrap_or(0),
                        last_event: stats.and_then(|s| s.last_event.clone()),
                        last_event_message: stats.and_then(|s| s.last_event_message.clone()),
                        session_id: stats.and_then(|s| s.session_id.clone()),
                        current_tool_name: stats.and_then(|s| s.current_tool_name.clone()),
                        current_tool_args_preview: stats
                            .and_then(|s| s.current_tool_args_preview.clone()),
                        last_error: self
                            .worker_session_info
                            .get(issue_id)
                            .and_then(|info| info.last_error.clone()),
                    },
                )
            })
            .collect();
        let running_session_info: BTreeMap<String, WorkerSessionInfo> = self
            .state
            .running
            .keys()
            .filter_map(|issue_id| {
                self.worker_session_info
                    .get(issue_id)
                    .map(|info| (issue_id.clone(), info.clone()))
            })
            .collect();

        let claimed: BTreeSet<String> = self.state.claimed.iter().cloned().collect();
        let mut completed: Vec<CompletedEntry> = self.state.completed.values().cloned().collect();
        completed.sort_by_key(|entry| std::cmp::Reverse(entry.completed_at));

        let mut retry_queue: Vec<RetrySnapshotEntry> = self
            .state
            .retry_attempts
            .values()
            .map(|entry| RetrySnapshotEntry {
                issue_id: entry.issue_id.clone(),
                identifier: entry.identifier.clone(),
                attempt: entry.attempt,
                due_in_ms: entry.due_at_ms - now_ms,
                error: entry.error.clone(),
                worker_host: entry.worker_host.clone(),
                workspace_path: entry.workspace_path.clone(),
            })
            .collect();

        retry_queue.sort_by(|a, b| {
            a.due_in_ms
                .cmp(&b.due_in_ms)
                .then_with(|| a.identifier.cmp(&b.identifier))
        });

        OrchestratorSnapshot {
            poll_interval_ms: self.state.poll_interval_ms,
            max_concurrent_agents: self.state.max_concurrent_agents,
            tracker_project_url: self.config.tracker.tracker_project_url(),
            running,
            running_sessions,
            blocked: self.blocked_issues.clone(),
            pending_escalations: self.escalation_registry.pending_snapshot(),
            shared_context: self.shared_context_store.summary(),
            supervisor: self.supervisor_snapshot(),
            running_session_info,
            claimed,
            retry_queue,
            completed,
            codex_totals: self.state.codex_totals.clone(),
            codex_rate_limits: self.state.codex_rate_limits.clone(),
            polling: PollingSnapshot {
                checking: false,
                next_poll_in_ms: self.state.poll_interval_ms as i64,
                poll_interval_ms: self.state.poll_interval_ms,
                last_poll_at: self.last_poll_at.map(|t| t.to_rfc3339()),
                poll_count: self.poll_count,
            },
        }
    }

    fn reconcile_running(&mut self, port: &mut dyn OrchestratorPort) -> Result<()> {
        let running_issue_ids: Vec<String> = self.state.running.keys().cloned().collect();
        let refreshed_issues = match port.reconcile_running_issues(&running_issue_ids) {
            Ok(issues) => issues,
            Err(err) => {
                tracing::warn!(
                    phase = "reconcile",
                    issue_count = running_issue_ids.len(),
                    error = %err,
                    "reconcile_running: failed to refresh running issues; keeping active workers"
                );
                return Ok(());
            }
        };

        let terminal_states = self.terminal_state_set();
        let active_states = self.active_state_set();
        let mut visible_issue_ids: HashSet<String> = HashSet::new();

        for issue in refreshed_issues {
            visible_issue_ids.insert(issue.id.clone());

            let normalized_state = normalize_issue_state(&issue.state);
            let previous_display = self.running_issue_states.get(&issue.id).cloned();

            if let Some(prev) = previous_display.as_deref() {
                let prev_normalized = normalize_issue_state(prev);
                if prev_normalized != normalized_state {
                    let event_name = normalized_state.replace(' ', "_");
                    let message = format!("Moved to {} (was {}).", issue.state, prev);
                    self.queue_slack_notification(
                        &event_name,
                        &issue.identifier,
                        &issue.title,
                        &message,
                        issue.url.as_deref(),
                    );
                }
            }

            if terminal_states.contains(&normalized_state) {
                self.maybe_write_completion_comment(port, &issue);
                self.mark_issue_terminal(&issue, None, true);
                continue;
            }

            if !issue.assigned_to_worker || !active_states.contains(&normalized_state) {
                self.release_issue(&issue.id);
                continue;
            }

            // Store display-cased state for human-readable notification messages.
            self.running_issue_states
                .insert(issue.id.clone(), issue.state.clone());
            self.running_parent_identifiers
                .insert(issue.id.clone(), issue.parent_identifier.clone());

            // Keep dashboard tracker_state current with actual tracker state.
            if let Some(attempt) = self.state.running.get_mut(&issue.id) {
                attempt.tracker_state = Some(issue.state.clone());
            }
        }

        for running_id in running_issue_ids {
            if !visible_issue_ids.contains(&running_id) {
                self.release_issue(&running_id);
            }
        }

        // Clean up stale running_issue_states entries for issues no longer
        // in state.running (completed workers whose state was kept for
        // notification detection on this poll cycle).
        self.running_issue_states
            .retain(|id, _| self.state.running.contains_key(id));
        self.running_parent_identifiers
            .retain(|id, _| self.state.running.contains_key(id));

        Ok(())
    }

    fn sort_issues_for_dispatch(&self, mut issues: Vec<Issue>) -> Vec<Issue> {
        issues.sort_by(|a, b| {
            priority_rank(a.priority)
                .cmp(&priority_rank(b.priority))
                .then_with(|| issue_created_at_sort_key(a).cmp(&issue_created_at_sort_key(b)))
                .then_with(|| issue_identifier_sort_key(a).cmp(&issue_identifier_sort_key(b)))
        });

        issues
    }

    /// Select a worker host from the SSH pool for the next dispatch attempt.
    ///
    /// - Returns `Local` when no SSH hosts are configured.
    /// - Returns `Remote(host)` with the preferred host when it is still under cap.
    /// - Returns `Remote(host)` with the least-loaded eligible host otherwise.
    /// - Returns `NoneAvailable` when all hosts are at or above the per-host cap.
    fn select_worker_host(&self, preferred: Option<&str>) -> WorkerHostSelection {
        if self.config.workspace.isolation == WorkspaceIsolation::Docker {
            return WorkerHostSelection::Local;
        }

        let ssh_hosts = &self.config.worker.ssh_hosts;
        let cap = self
            .config
            .worker
            .max_concurrent_agents_per_host
            .map(|c| c as usize)
            .unwrap_or(usize::MAX);

        let mut load: HashMap<String, usize> = HashMap::new();
        for attempt in self.state.running.values() {
            if let Some(host) = attempt.worker_host.as_deref() {
                *load.entry(host.to_string()).or_insert(0) += 1;
            }
        }

        ssh::select_worker_host(ssh_hosts, &load, cap, preferred)
    }

    /// Like `should_dispatch_issue` but without slot availability or claimed/running
    /// checks. Used by the first pass to identify blocked candidates for the TUI
    /// regardless of whether there are free slots or the issue is already queued.
    fn is_candidate_for_blocked_check(&self, issue: &Issue) -> bool {
        if !issue_has_required_fields(issue) {
            return false;
        }
        if !issue.assigned_to_worker {
            return false;
        }
        let normalized_state = normalize_issue_state(&issue.state);
        if self.terminal_state_set().contains(&normalized_state) {
            return false;
        }
        if !self.active_state_set().contains(&normalized_state) {
            return false;
        }
        if self.issue_has_excluded_label(issue) {
            return false;
        }
        true
    }

    fn should_dispatch_issue(&self, issue: &Issue) -> bool {
        if !issue_has_required_fields(issue) {
            return false;
        }

        if !issue.assigned_to_worker {
            return false;
        }

        let normalized_state = normalize_issue_state(&issue.state);

        if self.terminal_state_set().contains(&normalized_state) {
            return false;
        }

        if !self.active_state_set().contains(&normalized_state) {
            return false;
        }

        // Kata-shaped work uses parent slice issues to coordinate child tasks.
        // Child task issues must not be dispatched as independent workers; the
        // parent slice worker is responsible for executing them in order.
        if issue.parent_identifier.is_some() {
            return false;
        }

        // Skip issues carrying any of the configured exclude_labels.
        if self.issue_has_excluded_label(issue) {
            return false;
        }

        // NOTE: blocker checks are done at the dispatch loop level via
        // is_blocked_by_dependency() which needs access to all candidates.

        if self.state.claimed.contains(&issue.id) || self.state.running.contains_key(&issue.id) {
            return false;
        }

        if self.available_slots() == 0 {
            return false;
        }

        true
    }

    fn workspace_refresh_policy_for_dispatch(
        issue: &Issue,
        attempt: Option<u32>,
    ) -> workspace::ExistingWorkspaceRefreshPolicy {
        if attempt.is_some() || normalize_issue_state(&issue.state) != "todo" {
            workspace::ExistingWorkspaceRefreshPolicy::AllowStale
        } else {
            workspace::ExistingWorkspaceRefreshPolicy::Strict
        }
    }

    fn prepare_workspace_for_active_dispatch(
        &mut self,
        issue: &Issue,
        refresh_policy: workspace::ExistingWorkspaceRefreshPolicy,
    ) -> Option<WorkspaceDispatchPreparation> {
        let hook_cwd = self
            .workflow_store
            .as_ref()
            .map(|ws| ws.workflow_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        match workspace::ensure_workspace_for_issue_with_refresh_policy_and_hook_cwd(
            issue,
            &self.config.workspace,
            &self.config.hooks,
            refresh_policy,
            &hook_cwd,
        ) {
            Ok(info) => Some(WorkspaceDispatchPreparation {
                path: info.workspace.path,
                status_context: info
                    .refresh_notice
                    .as_ref()
                    .map(workspace::WorkspaceRefreshNotice::to_prompt_context),
            }),
            Err(err) => {
                let error = err.to_string();
                tracing::warn!(
                    event = "dispatch_workspace_prepare_failed",
                    issue_id = %issue.id,
                    issue_identifier = %issue.identifier,
                    error = %error,
                    "workspace preparation failed before dispatch; skipping issue for this tick"
                );
                self.emit_runtime_event(RuntimeEvent::WorkspacePrepareFailed {
                    issue_id: issue.id.clone(),
                    issue_identifier: issue.identifier.clone(),
                    error,
                });
                None
            }
        }
    }

    fn enforce_agent_review_pr_gate(
        &mut self,
        issue: &Issue,
        workspace_path: &str,
        port: &mut dyn OrchestratorPort,
    ) -> Result<Option<Issue>> {
        if normalize_issue_state(&issue.state) != "agent review" {
            return Ok(Some(issue.clone()));
        }

        let pr_status = check_agent_review_pr_status(
            Path::new(&workspace_path),
            self.config.workspace.base_branch.as_deref(),
        );

        let (branch, reason) = match pr_status {
            AgentReviewPrStatus::Valid { .. } => return Ok(Some(issue.clone())),
            AgentReviewPrStatus::CheckFailed { reason } => {
                tracing::warn!(
                    event = "agent_review_check_failed",
                    issue_id = %issue.id,
                    issue_identifier = %issue.identifier,
                    reason = %reason,
                    "PR gate check failed transiently; skipping issue without demotion"
                );
                return Ok(None);
            }
            AgentReviewPrStatus::Missing { branch, reason } => (branch, reason),
        };

        tracing::warn!(
            event = "agent_review_reset_missing_pr",
            issue_id = %issue.id,
            issue_identifier = %issue.identifier,
            workspace_path = %workspace_path,
            branch = branch.as_deref().unwrap_or("unknown"),
            reason = %reason,
            "agent review requires an open PR; moving issue back to In Progress"
        );

        port.update_issue_state(&issue.id, "In Progress")?;

        let note = invalid_agent_review_note(issue, branch.as_deref(), &reason);
        if let Err(err) = port.create_issue_comment(&issue.id, &note) {
            tracing::warn!(
                event = "agent_review_reset_comment_failed",
                issue_id = %issue.id,
                issue_identifier = %issue.identifier,
                error = %err,
                "failed to write agent review reset note"
            );
        }

        Ok(None)
    }

    /// Returns `true` if the issue carries at least one label that matches an
    /// entry in `tracker.exclude_labels` (comparison is case-insensitive).
    fn issue_has_excluded_label(&self, issue: &Issue) -> bool {
        let excluded: std::collections::HashSet<String> = self
            .config
            .tracker
            .exclude_labels
            .iter()
            .map(|l| l.trim().to_ascii_lowercase())
            .filter(|l| !l.is_empty())
            .collect();
        if excluded.is_empty() {
            return false;
        }
        issue
            .labels
            .iter()
            .map(|l| l.trim().to_ascii_lowercase())
            .filter(|l| !l.is_empty())
            .any(|l| excluded.contains(l.as_str()))
    }

    /// Returns `true` if the issue has at least one non-terminal blocker,
    /// meaning it should not be dispatched. Applies to **all** active states
    /// (not just Todo).
    ///
    /// Cross-project blockers (state = None) are treated as **non-blocking**
    /// with a warning, since Symphony cannot resolve them.
    ///
    /// When `candidate_ids` is provided, direct circular dependencies (A↔B)
    /// are detected and a warning is logged for observability. Note that the
    /// circular detection itself does not cause blocking — both issues are
    /// already blocked individually by the non-terminal blocker check above.
    fn is_blocked_by_dependency(
        &self,
        issue: &Issue,
        all_issues: &[Issue],
        candidate_ids: &std::collections::HashSet<String>,
    ) -> (bool, Vec<String>) {
        if issue.blocked_by.is_empty() {
            return (false, vec![]);
        }

        let terminal_states = self.terminal_state_set();
        let mut blocking_identifiers: Vec<String> = Vec::new();

        for blocker in &issue.blocked_by {
            let blocker_state = match &blocker.state {
                Some(s) => s,
                None => {
                    // Cross-project blocker — unknown state, treat as non-blocking
                    let blocker_id = blocker.identifier.as_deref().unwrap_or("unknown");
                    tracing::warn!(
                        event = "cross_project_blocker_ignored",
                        issue_id = %issue.id,
                        issue_identifier = %issue.identifier,
                        blocker_identifier = %blocker_id,
                        "blocker has no state info (cross-project?); treating as non-blocking"
                    );
                    continue;
                }
            };

            if terminal_states.contains(&normalize_issue_state(blocker_state)) {
                continue; // blocker resolved
            }

            // Non-terminal blocker — this issue is blocked
            let blocker_id = blocker
                .identifier
                .as_deref()
                .unwrap_or("unknown")
                .to_string();
            blocking_identifiers.push(blocker_id);
        }

        // Detect direct circular dependencies (A↔B) for observability.
        // NOTE: This block is purely informational — it does NOT cause blocking.
        // Both issues are already blocked naturally by the logic above: when A is
        // processed it sees B as a non-terminal blocker, and vice versa. The warning
        // simply makes circular relationships visible in logs for operators.
        if !blocking_identifiers.is_empty() {
            for blocker in &issue.blocked_by {
                if let Some(blocker_issue_id) = &blocker.id {
                    if candidate_ids.contains(blocker_issue_id) {
                        if let Some(blocker_issue) =
                            all_issues.iter().find(|i| i.id == *blocker_issue_id)
                        {
                            let reverse_blocked = blocker_issue
                                .blocked_by
                                .iter()
                                .any(|b| b.id.as_deref() == Some(&issue.id));
                            if reverse_blocked {
                                tracing::warn!(
                                    event = "circular_dependency_detected",
                                    issue_a = %issue.identifier,
                                    issue_b = %blocker_issue.identifier,
                                    "circular dependency detected between issues (both are \
                                     already blocked individually by the non-terminal blocker check above)"
                                );
                            }
                        }
                    }
                }
            }
        }

        let blocked = !blocking_identifiers.is_empty();
        if blocked {
            tracing::info!(
                event = "dispatch_blocked_by_dependency",
                issue_id = %issue.id,
                issue_identifier = %issue.identifier,
                blocker_identifiers = ?blocking_identifiers,
                "issue blocked by non-terminal dependencies; skipping dispatch"
            );
        }

        (blocked, blocking_identifiers)
    }

    fn available_slots(&self) -> u32 {
        self.state
            .max_concurrent_agents
            .saturating_sub(self.state.running.len() as u32)
    }

    fn retry_delay_ms(&self, retry_kind: RetryKind, attempt: u32) -> i64 {
        match retry_kind {
            RetryKind::Continuation => CONTINUATION_RETRY_DELAY_MS,
            RetryKind::Failure => {
                let max_backoff_ms =
                    self.config.agent.max_retry_backoff_ms.min(i64::MAX as u64) as i64;
                let safe_attempt = attempt.max(1);
                let power = safe_attempt.saturating_sub(1).min(10);
                let exponential = FAILURE_RETRY_BASE_MS.saturating_mul(1_i64 << power);
                exponential.min(max_backoff_ms)
            }
        }
    }

    fn dispatch_issue(
        &mut self,
        issue: &Issue,
        attempt: Option<u32>,
        workspace_path: Option<String>,
        worker_host: Option<String>,
    ) {
        let attempt = RunAttempt {
            issue_id: issue.id.clone(),
            issue_identifier: issue.identifier.clone(),
            issue_title: Some(issue.title.clone()),
            attempt,
            workspace_path: workspace_path
                .unwrap_or_else(|| self.default_workspace_path_for_issue(issue)),
            started_at: Utc::now(),
            status: "scheduled".to_string(),
            error: None,
            worker_host,
            model: if self.config.agent_backend == AgentBackend::KataCli {
                effective_pi_model_for_issue(&self.config, issue)
            } else {
                None
            },
            tracker_state: Some(issue.state.clone()),
            issue_url: issue.url.clone(),
        };

        self.completion_comment_summaries.remove(&issue.id);
        self.state.running.insert(issue.id.clone(), attempt);
        let _ = self.ensure_worker_session_info(&issue.id);
        self.state.claimed.insert(issue.id.clone());
        self.running_session_stats.insert(
            issue.id.clone(),
            RunningSessionStats {
                turn_count: 0,
                last_activity_at: Some(Utc::now()),
                total_tokens: 0,
                last_event: None,
                last_event_message: None,
                session_id: None,
                current_tool_name: None,
                current_tool_args_preview: None,
            },
        );
        self.state.retry_attempts.remove(&issue.id);
        self.running_issue_states
            .insert(issue.id.clone(), issue.state.clone());
        self.running_parent_identifiers
            .insert(issue.id.clone(), issue.parent_identifier.clone());
    }

    fn process_due_retries(
        &mut self,
        port: &mut dyn OrchestratorPort,
        now_ms: i64,
    ) -> Vec<DispatchedIssue> {
        let mut dispatched = Vec::new();
        let due_retries: Vec<RetryEntry> = self
            .state
            .retry_attempts
            .values()
            .filter(|entry| entry.due_at_ms <= now_ms)
            .cloned()
            .collect();

        for retry in due_retries {
            let Some(token) = retry.timer_handle.clone() else {
                continue;
            };

            if !self.fire_retry(&retry.issue_id, &token) {
                continue;
            }

            let retry_context = RetryContext {
                worker_host: retry.worker_host.clone(),
                workspace_path: retry.workspace_path.clone(),
                session_id: self.worker_session_ids.get(&retry.issue_id).cloned(),
            };

            let candidates = match port.fetch_candidate_issues() {
                Ok(issues) => issues,
                Err(err) => {
                    tracing::warn!(
                        event = "retry_poll_failed",
                        issue_id = %retry.issue_id,
                        issue_identifier = %retry.identifier,
                        error = %err,
                        "retry poll failed; rescheduling"
                    );

                    self.schedule_retry_with_context(
                        &retry.issue_id,
                        &retry.identifier,
                        retry.attempt.saturating_add(1),
                        RetryKind::Failure,
                        now_ms,
                        Some(format!("retry poll failed: {err}")),
                        retry_context,
                    );
                    continue;
                }
            };

            let Some(issue) = candidates
                .into_iter()
                .find(|issue| issue.id == retry.issue_id)
            else {
                let refreshed_issue = match port.refresh_issue(&retry.issue_id) {
                    Ok(issue) => issue,
                    Err(err) => {
                        tracing::warn!(
                            event = "retry_refresh_failed",
                            issue_id = %retry.issue_id,
                            issue_identifier = %retry.identifier,
                            error = %err,
                            "retry issue refresh failed; rescheduling"
                        );
                        self.schedule_retry_with_context(
                            &retry.issue_id,
                            &retry.identifier,
                            retry.attempt.saturating_add(1),
                            RetryKind::Failure,
                            now_ms,
                            Some(format!("retry refresh failed: {err}")),
                            retry_context,
                        );
                        continue;
                    }
                };

                if let Some(hidden_issue) = refreshed_issue {
                    let hidden_state = normalize_issue_state(&hidden_issue.state);
                    if self.terminal_state_set().contains(&hidden_state) {
                        tracing::debug!(
                            event = "retry_issue_terminal_after_refresh",
                            issue_id = %hidden_issue.id,
                            issue_identifier = %hidden_issue.identifier,
                            state = %hidden_state,
                            "retry issue became terminal before active-candidate visibility; marking terminal"
                        );
                        self.maybe_write_completion_comment(port, &hidden_issue);
                        self.mark_issue_terminal(
                            &hidden_issue,
                            retry.workspace_path.as_deref(),
                            true,
                        );
                        continue;
                    }
                }

                tracing::debug!(
                    event = "retry_issue_not_visible",
                    issue_id = %retry.issue_id,
                    issue_identifier = %retry.identifier,
                    "retry issue not visible in active candidates; releasing claim"
                );
                self.release_issue(&retry.issue_id);
                continue;
            };

            let workspace_refresh_policy =
                Self::workspace_refresh_policy_for_dispatch(&issue, Some(retry.attempt));
            let Some(workspace_preparation) =
                self.prepare_workspace_for_active_dispatch(&issue, workspace_refresh_policy)
            else {
                continue;
            };
            let workspace_path = workspace_preparation.path.clone();

            let Some(issue) =
                (match self.enforce_agent_review_pr_gate(&issue, &workspace_path, port) {
                    Ok(issue) => issue,
                    Err(err) => {
                        tracing::warn!(
                            event = "agent_review_gate_failed_retry",
                            issue_id = %issue.id,
                            issue_identifier = %issue.identifier,
                            error = %err,
                            "agent review PR gate failed during retry dispatch"
                        );
                        continue;
                    }
                })
            else {
                continue;
            };

            let normalized_state = normalize_issue_state(&issue.state);
            if self.terminal_state_set().contains(&normalized_state) {
                self.maybe_write_completion_comment(port, &issue);
                self.mark_issue_terminal(&issue, retry.workspace_path.as_deref(), true);
                continue;
            }

            if self.should_dispatch_issue(&issue) {
                // Select an SSH host for retry, preferring the prior attempt's host.
                let host_selection = self.select_worker_host(retry.worker_host.as_deref());
                if matches!(host_selection, WorkerHostSelection::NoneAvailable) {
                    tracing::warn!(
                        event = "ssh_pool_exhausted_retry",
                        issue_id = %issue.id,
                        issue_identifier = %issue.identifier,
                        "SSH host pool exhausted on retry, deferring"
                    );
                    // Reschedule at continuation delay WITHOUT incrementing attempt —
                    // pool exhaustion is transient capacity pressure, not a worker
                    // failure, so we must not consume retry budget or apply
                    // exponential backoff.
                    self.schedule_retry_with_context(
                        &issue.id,
                        &issue.identifier,
                        retry.attempt,
                        RetryKind::Continuation,
                        now_ms,
                        Some("ssh pool exhausted".to_string()),
                        retry_context,
                    );
                    continue;
                }
                let worker_host = match host_selection {
                    WorkerHostSelection::Remote(ref host) => Some(host.clone()),
                    _ => None,
                };
                self.dispatch_issue(
                    &issue,
                    Some(retry.attempt),
                    Some(workspace_path),
                    worker_host.clone(),
                );
                dispatched.push(DispatchedIssue {
                    issue,
                    attempt: Some(retry.attempt),
                    worker_host,
                    workspace_refresh_policy,
                    workspace_status_context: workspace_preparation.status_context,
                });
                continue;
            }

            if !self.active_state_set().contains(&normalized_state) {
                tracing::debug!(
                    event = "retry_issue_inactive",
                    issue_id = %issue.id,
                    issue_identifier = %issue.identifier,
                    state = %normalized_state,
                    "retry issue left active states; releasing claim"
                );
                self.release_issue(&issue.id);
                continue;
            }

            tracing::debug!(
                event = "retry_no_slots",
                issue_id = %issue.id,
                issue_identifier = %issue.identifier,
                "retry issue blocked by orchestrator slot constraints; rescheduling"
            );

            self.schedule_retry_with_context(
                &issue.id,
                &issue.identifier,
                retry.attempt.saturating_add(1),
                RetryKind::Failure,
                now_ms,
                Some("no available orchestrator slots".to_string()),
                retry_context,
            );
        }

        dispatched
    }

    fn default_workspace_path_for_issue(&self, issue: &Issue) -> String {
        let safe_identifier = path_safety::sanitize_identifier(&issue.identifier);
        Path::new(&self.config.workspace.root)
            .join(safe_identifier)
            .to_string_lossy()
            .to_string()
    }

    fn completion_comment_for_issue(&self, issue: &Issue, now: DateTime<Utc>) -> String {
        let run_attempt = self.state.running.get(&issue.id);
        let running_stats = self.running_session_stats.get(&issue.id);
        let session_info = self.worker_session_info.get(&issue.id);
        let summary = self.completion_comment_summaries.get(&issue.id);

        let turn_count = running_stats
            .map(|stats| stats.turn_count)
            .or_else(|| session_info.map(|info| info.turn_count.saturating_sub(1)))
            .or_else(|| summary.map(|cached| cached.turn_count))
            .unwrap_or_default();
        let total_tokens = running_stats
            .map(|stats| stats.total_tokens)
            .or_else(|| session_info.map(|info| info.session_tokens.total_tokens))
            .or_else(|| summary.map(|cached| cached.total_tokens))
            .unwrap_or_default();
        let duration = run_attempt
            .map(|attempt| now.signed_duration_since(attempt.started_at))
            .or_else(|| summary.map(|cached| cached.duration))
            .unwrap_or_else(chrono::Duration::zero);
        let worker_host = run_attempt
            .and_then(|attempt| attempt.worker_host.as_deref())
            .or_else(|| summary.and_then(|cached| cached.worker_host.as_deref()));

        CompletionCommentBuilder::new(
            &issue.identifier,
            &issue.state,
            turn_count,
            total_tokens,
            duration,
            worker_host,
        )
        .build()
    }

    fn maybe_write_completion_comment(&self, port: &mut dyn OrchestratorPort, issue: &Issue) {
        if !completion_comments_enabled(&self.config.tracker) {
            return;
        }

        let body = self.completion_comment_for_issue(issue, Utc::now());
        match port.create_issue_comment(&issue.id, &body) {
            Ok(()) => {
                tracing::info!(
                    event = "completion_comment_written",
                    issue_id = %issue.id,
                    issue_identifier = %issue.identifier,
                    tracker_kind = %self.config.tracker.kind.as_deref().unwrap_or("unknown"),
                    "wrote structured completion comment"
                );
            }
            Err(err) => {
                tracing::warn!(
                    event = "completion_comment_failed",
                    issue_id = %issue.id,
                    issue_identifier = %issue.identifier,
                    tracker_kind = %self.config.tracker.kind.as_deref().unwrap_or("unknown"),
                    error = %err,
                    "failed to write structured completion comment"
                );
            }
        }
    }

    fn mark_issue_terminal(
        &mut self,
        issue: &Issue,
        workspace_path_hint: Option<&str>,
        include_in_completed: bool,
    ) {
        let issue_id = issue.id.as_str();

        self.cancel_pending_escalations_for_issue(issue_id, "issue_terminal");

        if self.config.workspace.cleanup_on_done {
            let workspace_path = self
                .state
                .running
                .get(issue_id)
                .map(|attempt| attempt.workspace_path.clone())
                .or_else(|| {
                    self.state
                        .retry_attempts
                        .get(issue_id)
                        .and_then(|retry| retry.workspace_path.clone())
                })
                .or_else(|| workspace_path_hint.map(str::to_string));

            if let Some(workspace_path) = workspace_path {
                if self.worker_session_ids.contains_key(issue_id) {
                    self.pending_terminal_cleanup.insert(
                        issue_id.to_string(),
                        PendingTerminalCleanup {
                            issue: issue.clone(),
                            workspace_path,
                        },
                    );
                    tracing::info!(
                        event = "terminal_workspace_cleanup_deferred_active_worker",
                        issue_id = %issue_id,
                        issue_identifier = %issue.identifier,
                        "deferring workspace cleanup until worker completion"
                    );
                } else {
                    self.pending_terminal_cleanup.remove(issue_id);
                    self.cleanup_workspace(issue, &workspace_path);
                }
            }
        }

        if include_in_completed {
            self.state.completed.insert(
                issue_id.to_string(),
                CompletedEntry {
                    issue_id: issue_id.to_string(),
                    identifier: issue.identifier.clone(),
                    title: issue.title.clone(),
                    completed_at: None,
                },
            );
        } else {
            self.state.completed.remove(issue_id);
        }
        self.state.running.remove(issue_id);
        self.state.claimed.remove(issue_id);
        self.state.retry_attempts.remove(issue_id);
        self.running_issue_states.remove(issue_id);
        self.running_parent_identifiers.remove(issue_id);
        self.retry_tokens.remove(issue_id);
        self.worker_last_activity_ms.remove(issue_id);
        self.worker_session_info.remove(issue_id);
        self.worker_session_ids.remove(issue_id);
        self.worker_steer_tx.remove(issue_id);
        self.running_session_stats.remove(issue_id);
        self.completion_comment_summaries.remove(issue_id);
    }

    fn cleanup_workspace(&self, issue: &Issue, workspace_path: &str) {
        let workspace = Path::new(workspace_path);
        if !workspace.exists() {
            tracing::debug!(
                event = "terminal_workspace_cleanup_skipped_missing_path",
                issue_id = %issue.id,
                issue_identifier = %issue.identifier,
                workspace_path = %workspace.display(),
                "workspace cleanup skipped because path does not exist"
            );
            return;
        }

        let hook_cwd = self
            .workflow_store
            .as_ref()
            .map(|ws| ws.workflow_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        match workspace::remove_workspace_for_issue_with_hook_cwd(
            workspace,
            &self.config.workspace,
            &self.config.hooks,
            issue,
            &hook_cwd,
        ) {
            Ok(()) => {
                tracing::info!(
                    event = "terminal_workspace_cleanup_succeeded",
                    issue_id = %issue.id,
                    issue_identifier = %issue.identifier,
                    workspace_path = %workspace.display(),
                    "removed workspace after issue reached terminal state"
                );
            }
            Err(err) => {
                tracing::warn!(
                    event = "terminal_workspace_cleanup_failed",
                    issue_id = %issue.id,
                    issue_identifier = %issue.identifier,
                    workspace_path = %workspace.display(),
                    error = %err,
                    "workspace cleanup failed; continuing terminal transition"
                );
            }
        }
    }

    fn release_issue(&mut self, issue_id: &str) {
        self.cancel_pending_escalations_for_issue(issue_id, "issue_released");

        self.state.running.remove(issue_id);
        self.state.claimed.remove(issue_id);
        self.state.retry_attempts.remove(issue_id);
        self.running_issue_states.remove(issue_id);
        self.running_parent_identifiers.remove(issue_id);
        self.retry_tokens.remove(issue_id);
        self.worker_last_activity_ms.remove(issue_id);
        self.worker_session_ids.remove(issue_id);
        self.worker_steer_tx.remove(issue_id);
        self.running_session_stats.remove(issue_id);
        self.completion_comment_summaries.remove(issue_id);
    }

    fn active_state_set(&self) -> HashSet<String> {
        self.config
            .tracker
            .active_states
            .iter()
            .map(|state| normalize_issue_state(state))
            .filter(|state| !state.is_empty())
            .collect()
    }

    fn terminal_state_set(&self) -> HashSet<String> {
        self.config
            .tracker
            .terminal_states
            .iter()
            .map(|state| normalize_issue_state(state))
            .filter(|state| !state.is_empty())
            .collect()
    }
}

fn event_timestamp_ms(event: &AgentEvent) -> i64 {
    event_timestamp(event).timestamp_millis()
}

impl Drop for Orchestrator {
    fn drop(&mut self) {
        if let Some(supervisor) = self.supervisor_agent.as_mut() {
            supervisor.abort();
        }
    }
}

fn event_timestamp(event: &AgentEvent) -> DateTime<Utc> {
    match event {
        AgentEvent::SessionStarted { timestamp, .. }
        | AgentEvent::StartupFailed { timestamp, .. }
        | AgentEvent::TurnCompleted { timestamp, .. }
        | AgentEvent::TurnFailed { timestamp, .. }
        | AgentEvent::TurnCancelled { timestamp, .. }
        | AgentEvent::TurnEndedWithError { timestamp, .. }
        | AgentEvent::TurnInputRequired { timestamp, .. }
        | AgentEvent::ApprovalAutoApproved { timestamp, .. }
        | AgentEvent::ApprovalRequired { timestamp, .. }
        | AgentEvent::ToolCallCompleted { timestamp, .. }
        | AgentEvent::ToolCallFailed { timestamp, .. }
        | AgentEvent::ToolInputAutoAnswered { timestamp, .. }
        | AgentEvent::UnsupportedToolCall { timestamp, .. }
        | AgentEvent::EscalationCreated { timestamp, .. }
        | AgentEvent::EscalationResponded { timestamp, .. }
        | AgentEvent::EscalationTimedOut { timestamp, .. }
        | AgentEvent::EscalationCancelled { timestamp, .. }
        | AgentEvent::Notification { timestamp, .. }
        | AgentEvent::OtherMessage { timestamp, .. }
        | AgentEvent::Malformed { timestamp, .. } => *timestamp,
    }
}

fn event_session_id(event: &AgentEvent) -> Option<&str> {
    match event {
        AgentEvent::SessionStarted { session_id, .. } => Some(session_id.as_str()),
        _ => None,
    }
}

fn event_name(event: &AgentEvent) -> &'static str {
    match event {
        AgentEvent::SessionStarted { .. } => "session_started",
        AgentEvent::StartupFailed { .. } => "startup_failed",
        AgentEvent::TurnCompleted { .. } => "turn_completed",
        AgentEvent::TurnFailed { .. } => "turn_failed",
        AgentEvent::TurnCancelled { .. } => "turn_cancelled",
        AgentEvent::TurnEndedWithError { .. } => "turn_ended_with_error",
        AgentEvent::TurnInputRequired { .. } => "turn_input_required",
        AgentEvent::ApprovalAutoApproved { .. } => "approval_auto_approved",
        AgentEvent::ApprovalRequired { .. } => "approval_required",
        AgentEvent::ToolCallCompleted { .. } => "tool_call_completed",
        AgentEvent::ToolCallFailed { .. } => "tool_call_failed",
        AgentEvent::ToolInputAutoAnswered { .. } => "tool_input_auto_answered",
        AgentEvent::UnsupportedToolCall { .. } => "unsupported_tool_call",
        AgentEvent::EscalationCreated { .. } => "escalation_created",
        AgentEvent::EscalationResponded { .. } => "escalation_responded",
        AgentEvent::EscalationTimedOut { .. } => "escalation_timed_out",
        AgentEvent::EscalationCancelled { .. } => "escalation_cancelled",
        AgentEvent::Notification { .. } => "notification",
        AgentEvent::OtherMessage { .. } => "other_message",
        AgentEvent::Malformed { .. } => "malformed",
    }
}

fn event_kind_for_agent_event(event: &AgentEvent) -> EventKind {
    match event {
        AgentEvent::ToolCallCompleted { .. }
        | AgentEvent::ToolCallFailed { .. }
        | AgentEvent::UnsupportedToolCall { .. }
        | AgentEvent::ToolInputAutoAnswered { .. } => EventKind::Tool,
        AgentEvent::EscalationCreated { .. } => EventKind::EscalationCreated,
        AgentEvent::EscalationResponded { .. } => EventKind::EscalationResponded,
        AgentEvent::EscalationTimedOut { .. } => EventKind::EscalationTimedOut,
        AgentEvent::EscalationCancelled { .. } => EventKind::EscalationCancelled,
        AgentEvent::Notification { message, .. } if parse_tool_notification(message).is_some() => {
            EventKind::Tool
        }
        _ => EventKind::Worker,
    }
}

fn event_severity_for_agent_event(event: &AgentEvent) -> EventSeverity {
    match event {
        AgentEvent::StartupFailed { .. }
        | AgentEvent::TurnFailed { .. }
        | AgentEvent::TurnEndedWithError { .. }
        | AgentEvent::ToolCallFailed { .. }
        | AgentEvent::Malformed { .. } => EventSeverity::Error,
        AgentEvent::Notification { message, .. }
            if parse_tool_notification(message)
                .is_some_and(|notification| notification.event_name == "tool_error") =>
        {
            EventSeverity::Error
        }
        AgentEvent::TurnCancelled { .. }
        | AgentEvent::UnsupportedToolCall { .. }
        | AgentEvent::EscalationTimedOut { .. }
        | AgentEvent::EscalationCancelled { .. } => EventSeverity::Warn,
        AgentEvent::OtherMessage { .. } => EventSeverity::Debug,
        _ => EventSeverity::Info,
    }
}

fn format_rate_limit_error(error: &str) -> String {
    let normalized = error.to_ascii_lowercase();
    let is_rate_limit = normalized.contains("rate limit") || normalized.contains("usage limit");

    if !is_rate_limit {
        return truncate_for_display(error, WORKER_LAST_ERROR_MAX_CHARS);
    }

    if let Some(minutes) = extract_retry_window_minutes(error) {
        return format!("rate limit: retry in ~{minutes} min");
    }

    let formatted = format!("rate limit: {error}");
    truncate_for_display(&formatted, WORKER_LAST_ERROR_MAX_CHARS)
}

fn extract_retry_window_minutes(message: &str) -> Option<u64> {
    let mut total_seconds: u64 = 0;
    let mut matched = false;

    for captures in RATE_LIMIT_WINDOW_RE.captures_iter(message) {
        let Some(amount_match) = captures.get(1) else {
            continue;
        };
        let Some(unit_match) = captures.get(2) else {
            continue;
        };

        let Ok(amount) = amount_match.as_str().parse::<u64>() else {
            continue;
        };

        let next_char = message[unit_match.end()..].chars().next();
        if next_char.is_some_and(|ch| ch.is_ascii_alphabetic()) {
            continue;
        }

        let unit = unit_match.as_str().to_ascii_lowercase();
        let seconds = if unit.starts_with('h') {
            amount.saturating_mul(3_600)
        } else if unit.starts_with('m') {
            amount.saturating_mul(60)
        } else {
            amount
        };

        total_seconds = total_seconds.saturating_add(seconds);
        matched = true;
    }

    if !matched {
        return None;
    }

    Some(((total_seconds.saturating_add(59)) / 60).max(1))
}

fn event_summary(event: &AgentEvent) -> (String, Option<String>) {
    let (name, message) = match event {
        AgentEvent::SessionStarted { session_id, .. } => (
            event_name(event).to_string(),
            Some(format!("session {}", compact_session_id(session_id))),
        ),
        AgentEvent::StartupFailed { error, .. } => {
            (event_name(event).to_string(), Some(error.clone()))
        }
        AgentEvent::TurnCompleted { message, .. } => (
            event_name(event).to_string(),
            Some(
                message
                    .clone()
                    .unwrap_or_else(|| "turn completed".to_string()),
            ),
        ),
        AgentEvent::TurnFailed { error, .. } => {
            (event_name(event).to_string(), Some(error.clone()))
        }
        AgentEvent::TurnCancelled { .. } => (
            event_name(event).to_string(),
            Some("turn cancelled".to_string()),
        ),
        AgentEvent::TurnEndedWithError { error, .. } => {
            (event_name(event).to_string(), Some(error.clone()))
        }
        AgentEvent::TurnInputRequired { prompt, .. } => (
            event_name(event).to_string(),
            prompt
                .clone()
                .or_else(|| Some("input required".to_string())),
        ),
        AgentEvent::ApprovalAutoApproved { tool_call, .. } => (
            event_name(event).to_string(),
            Some(format!("auto-approved {tool_call}")),
        ),
        AgentEvent::ApprovalRequired { method, .. } => (
            event_name(event).to_string(),
            Some(format!("approval required: {method}")),
        ),
        AgentEvent::ToolCallCompleted { tool_name, .. } => (
            event_name(event).to_string(),
            Some(format!("completed {tool_name}")),
        ),
        AgentEvent::ToolCallFailed { tool_name, .. } => {
            let message = tool_name
                .as_ref()
                .map(|name| format!("tool {name} failed"))
                .unwrap_or_else(|| "tool call failed".to_string());
            (event_name(event).to_string(), Some(message))
        }
        AgentEvent::ToolInputAutoAnswered { .. } => (
            event_name(event).to_string(),
            Some("tool input auto-answered".to_string()),
        ),
        AgentEvent::UnsupportedToolCall { tool_name, .. } => (
            event_name(event).to_string(),
            Some(format!("unsupported tool {tool_name}")),
        ),
        AgentEvent::EscalationCreated { request, .. } => (
            event_name(event).to_string(),
            Some(format!(
                "{} needs input: {}",
                request.issue_identifier,
                truncate_for_display(&request.method, 80)
            )),
        ),
        AgentEvent::EscalationResponded {
            request_id,
            responder_id,
            latency_ms,
            ..
        } => (
            event_name(event).to_string(),
            Some(format!(
                "request {request_id} responded by {} in {}ms",
                responder_id.as_deref().unwrap_or("operator"),
                latency_ms
            )),
        ),
        AgentEvent::EscalationTimedOut {
            request_id,
            timeout_ms,
            ..
        } => (
            event_name(event).to_string(),
            Some(format!(
                "request {request_id} timed out after {timeout_ms}ms"
            )),
        ),
        AgentEvent::EscalationCancelled {
            request_id, reason, ..
        } => (
            event_name(event).to_string(),
            Some(format!("request {request_id} cancelled: {reason}")),
        ),
        AgentEvent::Notification { message, .. } => {
            if let Some(notification) = parse_tool_notification(message) {
                (notification.event_name.to_string(), notification.summary)
            } else {
                notification_event_summary(message)
            }
        }
        AgentEvent::OtherMessage { raw, .. } => other_message_summary(raw),
        AgentEvent::Malformed {
            parse_error,
            raw_text,
            ..
        } => (
            event_name(event).to_string(),
            Some(format!(
                "malformed event: {parse_error}; {}",
                normalize_whitespace(raw_text)
            )),
        ),
    };

    (
        name,
        message
            .as_deref()
            .map(|value| truncate_for_display(value, 160))
            .filter(|value| !value.is_empty()),
    )
}

fn notification_event_summary(message: &str) -> (String, Option<String>) {
    let fallback_message = normalize_whitespace(message);
    let parsed = match serde_json::from_str::<serde_json::Value>(message) {
        Ok(parsed) => parsed,
        Err(_) => {
            return (
                "notification".to_string(),
                (!fallback_message.is_empty()).then_some(fallback_message),
            )
        }
    };

    let name = parsed
        .get("method")
        .and_then(|method| method.as_str())
        .unwrap_or("notification")
        .to_string();
    let summary = summarize_notification_payload(&name, &parsed).or_else(|| {
        parsed
            .get("params")
            .map(|params| normalize_whitespace(&params.to_string()))
    });

    (name, summary)
}

fn summarize_notification_payload(name: &str, payload: &serde_json::Value) -> Option<String> {
    if name.contains("token_count") {
        let input = first_u64_at_paths(
            payload,
            &[
                &["params", "tokenUsage", "total", "input_tokens"],
                &["params", "tokenUsage", "total", "inputTokens"],
                &[
                    "params",
                    "msg",
                    "payload",
                    "info",
                    "total_token_usage",
                    "input_tokens",
                ],
            ],
        );
        let output = first_u64_at_paths(
            payload,
            &[
                &["params", "tokenUsage", "total", "output_tokens"],
                &["params", "tokenUsage", "total", "outputTokens"],
                &[
                    "params",
                    "msg",
                    "payload",
                    "info",
                    "total_token_usage",
                    "output_tokens",
                ],
            ],
        );
        let total = first_u64_at_paths(
            payload,
            &[
                &["params", "tokenUsage", "total", "total_tokens"],
                &["params", "tokenUsage", "total", "totalTokens"],
                &[
                    "params",
                    "msg",
                    "payload",
                    "info",
                    "total_token_usage",
                    "total_tokens",
                ],
            ],
        );

        let mut pieces = Vec::new();
        if let Some(value) = input {
            pieces.push(format!("in {value}"));
        }
        if let Some(value) = output {
            pieces.push(format!("out {value}"));
        }
        if let Some(value) = total {
            pieces.push(format!("total {value}"));
        }
        if !pieces.is_empty() {
            return Some(format!("tokens {}", pieces.join(" / ")));
        }
    }

    payload
        .get("params")
        .and_then(find_preferred_text)
        .or_else(|| find_preferred_text(payload))
}

fn other_message_summary(raw: &serde_json::Value) -> (String, Option<String>) {
    let name = raw
        .get("method")
        .and_then(|method| method.as_str())
        .unwrap_or("other_message")
        .to_string();
    let summary = raw
        .get("params")
        .and_then(find_preferred_text)
        .or_else(|| find_preferred_text(raw));
    (name, summary)
}

fn find_preferred_text(value: &serde_json::Value) -> Option<String> {
    const MAX_TEXT_SEARCH_DEPTH: usize = 5;
    find_preferred_text_with_depth(value, MAX_TEXT_SEARCH_DEPTH)
}

fn find_preferred_text_with_depth(value: &serde_json::Value, depth: usize) -> Option<String> {
    if depth == 0 {
        return None;
    }

    match value {
        serde_json::Value::String(text) => {
            let normalized = normalize_whitespace(text);
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        }
        serde_json::Value::Object(map) => {
            for key in [
                "summary",
                "message",
                "text",
                "title",
                "command",
                "tool_name",
                "toolName",
                "name",
            ] {
                if let Some(found) = map
                    .get(key)
                    .and_then(|candidate| find_preferred_text_with_depth(candidate, depth - 1))
                {
                    return Some(found);
                }
            }

            map.values()
                .find_map(|candidate| find_preferred_text_with_depth(candidate, depth - 1))
        }
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|candidate| find_preferred_text_with_depth(candidate, depth - 1)),
        _ => None,
    }
}

fn first_u64_at_paths(value: &serde_json::Value, paths: &[&[&str]]) -> Option<u64> {
    paths
        .iter()
        .find_map(|path| value_at_path(value, path).and_then(integer_like))
}

fn value_at_path<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for segment in path {
        current = current.get(segment)?;
    }
    Some(current)
}

fn integer_like(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn normalize_issue_state(state_name: &str) -> String {
    state_name.trim().to_ascii_lowercase()
}

fn issue_has_required_fields(issue: &Issue) -> bool {
    !issue.id.trim().is_empty()
        && !issue.identifier.trim().is_empty()
        && !issue.title.trim().is_empty()
        && !issue.state.trim().is_empty()
}

fn priority_rank(priority: Option<i32>) -> i32 {
    match priority {
        Some(value) if (1..=4).contains(&value) => value,
        _ => 5,
    }
}

fn issue_created_at_sort_key(issue: &Issue) -> i64 {
    issue
        .created_at
        .map(|created_at| created_at.timestamp_micros())
        .unwrap_or(i64::MAX)
}

fn issue_identifier_sort_key(issue: &Issue) -> (&str, &str) {
    (issue.identifier.as_str(), issue.id.as_str())
}

pub fn rate_limit_info(data: serde_json::Value) -> RateLimitInfo {
    RateLimitInfo { data }
}

// ── Tool activity extraction ──────────────────────────────────────────

/// Represents the current tool activity state derived from an agent event.
enum ToolActivity {
    /// A tool started executing.
    Started {
        name: String,
        args_preview: Option<String>,
    },
    /// A tool finished executing.
    Ended,
    /// Event is unrelated to tool activity.
    None,
}

/// Maximum length for the tool args preview string.
const TOOL_ARGS_PREVIEW_MAX_LEN: usize = 120;

/// Parsed representation of a `tool_*` notification message.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedToolNotification {
    event_name: &'static str,
    summary: Option<String>,
    tool_name: Option<String>,
    args_preview: Option<String>,
}

/// Extract tool activity information from an agent event.
///
/// Handles both Codex backend events (ToolCallCompleted/Failed) and
/// pi-agent RPC events (Notification messages with tool_start/tool_end prefixes).
fn extract_tool_activity(event: &AgentEvent) -> ToolActivity {
    match event {
        // Codex backend: tool calls complete in a single event (no start/end separation).
        // We don't set "started" for these since they arrive post-completion.
        AgentEvent::ToolCallCompleted { .. }
        | AgentEvent::ToolCallFailed { .. }
        | AgentEvent::UnsupportedToolCall { .. }
        | AgentEvent::TurnCompleted { .. }
        | AgentEvent::TurnFailed { .. }
        | AgentEvent::TurnCancelled { .. }
        | AgentEvent::TurnEndedWithError { .. } => ToolActivity::Ended,

        // Pi-agent RPC: tool execution events arrive as Notification messages.
        AgentEvent::Notification { message, .. } => parse_tool_notification(message)
            .map_or(ToolActivity::None, tool_activity_from_notification),

        _ => ToolActivity::None,
    }
}

fn tool_activity_from_notification(notification: ParsedToolNotification) -> ToolActivity {
    match notification.event_name {
        "tool_start" => {
            if let Some(name) = notification.tool_name {
                ToolActivity::Started {
                    name,
                    args_preview: notification.args_preview,
                }
            } else {
                ToolActivity::None
            }
        }
        "tool_end" | "tool_error" => ToolActivity::Ended,
        _ => ToolActivity::None,
    }
}

/// Parse a pi-agent notification message for tool metadata.
///
/// Messages follow the format:
/// - `"tool_start: <name> <args_json>"` — tool began executing
/// - `"tool_end: <name>"` — tool finished successfully
/// - `"tool_error: <name>"` — tool finished with error
fn parse_tool_notification(message: &str) -> Option<ParsedToolNotification> {
    for (prefix, event_name) in [
        ("tool_start:", "tool_start"),
        ("tool_end:", "tool_end"),
        ("tool_error:", "tool_error"),
    ] {
        let Some(rest) = message.strip_prefix(prefix) else {
            continue;
        };

        let rest = rest.trim_start();
        let summary = normalize_whitespace(rest);
        let summary = (!summary.is_empty()).then_some(summary);

        if event_name == "tool_start" {
            let mut parts = rest.splitn(2, char::is_whitespace);
            let tool_name = parts
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            let args_preview = parts
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(build_tool_args_preview)
                .map(|preview| truncate_for_display(&preview, TOOL_ARGS_PREVIEW_MAX_LEN));

            return Some(ParsedToolNotification {
                event_name,
                summary,
                tool_name,
                args_preview,
            });
        }

        let tool_name = summary.clone();
        return Some(ParsedToolNotification {
            event_name,
            summary,
            tool_name,
            args_preview: None,
        });
    }

    None
}

/// Build a human-readable preview of tool arguments from a JSON string.
///
/// For common tools, extracts the most meaningful argument:
/// - `bash`: shows the command
/// - `read`/`write`/`edit`: shows the path
/// - `browser_navigate`: shows the URL
/// - Others: shows a compact summary of top-level keys
fn build_tool_args_preview(args_json: &str) -> String {
    let parsed: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(_) => return args_json.chars().filter(|c| !c.is_control()).collect(),
    };

    let obj = match parsed.as_object() {
        Some(o) => o,
        None => return args_json.to_string(),
    };

    // Extract the most meaningful field for common tools
    if let Some(cmd) = obj.get("command").and_then(|v| v.as_str()) {
        return cmd.to_string();
    }
    if let Some(path) = obj.get("path").and_then(|v| v.as_str()) {
        return path.to_string();
    }
    if let Some(url) = obj.get("url").and_then(|v| v.as_str()) {
        return url.to_string();
    }
    if let Some(query) = obj.get("query").and_then(|v| v.as_str()) {
        return query.to_string();
    }

    // Fallback: show keys
    let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
    if keys.is_empty() {
        return "{}".to_string();
    }
    keys.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn workflow_dir_from_path_handles_root_level_workflow() {
        assert_eq!(
            workflow_dir_from_path(Path::new("WORKFLOW.md")),
            PathBuf::from(".")
        );
        assert_eq!(
            workflow_dir_from_path(Path::new(".symphony/WORKFLOW.md")),
            PathBuf::from(".symphony")
        );
    }

    #[test]
    fn find_preferred_text_stops_searching_past_depth_limit() {
        let nested = json!({
            "level_1": {
                "level_2": {
                    "level_3": {
                        "level_4": {
                            "level_5": {
                                "message": "too deep"
                            }
                        }
                    }
                }
            }
        });

        assert_eq!(find_preferred_text(&nested), None);
    }

    #[tokio::test]
    async fn create_event_hub_is_idempotent() {
        let mut orchestrator = Orchestrator::new(Default::default(), "prompt".to_string());
        let first = orchestrator.create_event_hub();
        let mut first_rx = first.subscribe();

        let second = orchestrator.create_event_hub();
        second.publish(
            EventKind::Worker,
            EventSeverity::Info,
            Some("KAT-1".to_string()),
            "worker_started",
            serde_json::json!({ "attempt": 1 }),
        );

        let envelope = tokio::time::timeout(std::time::Duration::from_secs(1), first_rx.recv())
            .await
            .expect("first receiver should observe events from second hub")
            .expect("event should decode");

        assert_eq!(envelope.event, "worker_started");
    }

    #[test]
    fn parse_tool_notification_start_with_args() {
        let msg = r#"tool_start: bash {"command":"cargo test","timeout":60}"#;
        let parsed = parse_tool_notification(msg).expect("notification should parse");

        assert_eq!(parsed.event_name, "tool_start");
        assert_eq!(parsed.tool_name.as_deref(), Some("bash"));
        assert_eq!(parsed.args_preview.as_deref(), Some("cargo test"));
        assert_eq!(
            parsed.summary.as_deref(),
            Some("bash {\"command\":\"cargo test\",\"timeout\":60}")
        );
    }

    #[test]
    fn parse_tool_notification_start_no_args() {
        let parsed =
            parse_tool_notification("tool_start: read").expect("notification should parse");

        assert_eq!(parsed.event_name, "tool_start");
        assert_eq!(parsed.tool_name.as_deref(), Some("read"));
        assert!(parsed.args_preview.is_none());
        assert_eq!(parsed.summary.as_deref(), Some("read"));
    }

    #[test]
    fn parse_tool_notification_end() {
        let parsed = parse_tool_notification("tool_end: bash").expect("notification should parse");

        assert_eq!(parsed.event_name, "tool_end");
        assert_eq!(parsed.summary.as_deref(), Some("bash"));
        assert!(matches!(
            tool_activity_from_notification(parsed),
            ToolActivity::Ended
        ));
    }

    #[test]
    fn parse_tool_notification_error() {
        let parsed =
            parse_tool_notification("tool_error: bash").expect("notification should parse");

        assert_eq!(parsed.event_name, "tool_error");
        assert_eq!(parsed.summary.as_deref(), Some("bash"));
        assert!(matches!(
            tool_activity_from_notification(parsed),
            ToolActivity::Ended
        ));
    }

    #[test]
    fn parse_tool_notification_unrelated() {
        assert!(parse_tool_notification("some other message").is_none());
    }

    #[test]
    fn build_tool_args_preview_extracts_command() {
        let preview = build_tool_args_preview(r#"{"command":"cargo test --release"}"#);
        assert_eq!(preview, "cargo test --release");
    }

    #[test]
    fn build_tool_args_preview_extracts_path() {
        let preview = build_tool_args_preview(r#"{"path":"src/main.rs","offset":10}"#);
        assert_eq!(preview, "src/main.rs");
    }

    #[test]
    fn build_tool_args_preview_fallback_to_keys() {
        let preview = build_tool_args_preview("{\"selector\":\"btn\",\"text\":\"click\"}");
        assert_eq!(preview, "selector, text");
    }

    #[test]
    fn build_tool_args_preview_invalid_json() {
        let preview = build_tool_args_preview("not json");
        assert_eq!(preview, "not json");
    }

    #[test]
    fn build_tool_args_preview_strips_control_chars_on_invalid_json() {
        let preview = build_tool_args_preview("bad\x00json\nwith\tcontrol");
        assert_eq!(preview, "badjsonwithcontrol"); // all control chars stripped
    }

    #[test]
    fn extract_tool_activity_clears_on_turn_completed() {
        let event = AgentEvent::TurnCompleted {
            timestamp: chrono::Utc::now(),
            codex_app_server_pid: None,
            turn_id: "t1".to_string(),
            message: None,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            rate_limits: None,
        };
        assert!(matches!(extract_tool_activity(&event), ToolActivity::Ended));
    }

    #[test]
    fn extract_tool_activity_clears_on_turn_failed() {
        let event = AgentEvent::TurnFailed {
            timestamp: chrono::Utc::now(),
            codex_app_server_pid: None,
            turn_id: "t1".to_string(),
            error: "crash".to_string(),
        };
        assert!(matches!(extract_tool_activity(&event), ToolActivity::Ended));
    }

    #[test]
    fn agent_review_pr_status_is_valid_when_branch_and_open_pr_match() {
        let workspace = Path::new("/tmp/workspace");
        let status = check_agent_review_pr_status_with(
            workspace,
            Some("main"),
            |program, args, _cwd| {
                match (program, args) {
                ("git", ["branch", "--show-current"]) => Ok(CommandOutcome {
                    success: true,
                    stdout: "sym/KAT-2499".to_string(),
                    stderr: String::new(),
                }),
                ("git", ["ls-remote", "--exit-code", "--heads", "origin", "sym/KAT-2499"]) => Ok(CommandOutcome {
                    success: true,
                    stdout: "abc123\trefs/heads/sym/KAT-2499".to_string(),
                    stderr: String::new(),
                }),
                ("gh", ["pr", "view", "--json", "url,state,headRefName,baseRefName"]) => Ok(CommandOutcome {
                    success: true,
                    stdout: r#"{"url":"https://github.com/gannonh/kata/pull/999","state":"OPEN","headRefName":"sym/KAT-2499","baseRefName":"main"}"#.to_string(),
                    stderr: String::new(),
                }),
                _ => panic!("unexpected command: {} {:?}", program, args),
            }
            },
        );

        assert_eq!(
            status,
            AgentReviewPrStatus::Valid {
                branch: "sym/KAT-2499".to_string(),
                pr_url: "https://github.com/gannonh/kata/pull/999".to_string(),
            }
        );
    }

    #[test]
    fn agent_review_pr_status_reports_missing_pr_for_branch() {
        let workspace = Path::new("/tmp/workspace");
        let status = check_agent_review_pr_status_with(
            workspace,
            Some("main"),
            |program, args, _cwd| match (program, args) {
                ("git", ["branch", "--show-current"]) => Ok(CommandOutcome {
                    success: true,
                    stdout: "sym/KAT-2499".to_string(),
                    stderr: String::new(),
                }),
                ("git", ["ls-remote", "--exit-code", "--heads", "origin", "sym/KAT-2499"]) => {
                    Ok(CommandOutcome {
                        success: true,
                        stdout: "abc123\trefs/heads/sym/KAT-2499".to_string(),
                        stderr: String::new(),
                    })
                }
                ("gh", ["pr", "view", "--json", "url,state,headRefName,baseRefName"]) => {
                    Ok(CommandOutcome {
                        success: false,
                        stdout: String::new(),
                        stderr: "no pull requests found for branch \"sym/KAT-2499\"".to_string(),
                    })
                }
                _ => panic!("unexpected command: {} {:?}", program, args),
            },
        );

        assert_eq!(
            status,
            AgentReviewPrStatus::Missing {
                branch: Some("sym/KAT-2499".to_string()),
                reason: "no open PR found for current branch `sym/KAT-2499`".to_string(),
            }
        );
    }

    #[test]
    fn agent_review_pr_status_rejects_closed_prs() {
        let workspace = Path::new("/tmp/workspace");
        let status = check_agent_review_pr_status_with(
            workspace,
            Some("main"),
            |program, args, _cwd| {
                match (program, args) {
                ("git", ["branch", "--show-current"]) => Ok(CommandOutcome {
                    success: true,
                    stdout: "sym/KAT-2499".to_string(),
                    stderr: String::new(),
                }),
                ("git", ["ls-remote", "--exit-code", "--heads", "origin", "sym/KAT-2499"]) => Ok(CommandOutcome {
                    success: true,
                    stdout: "abc123\trefs/heads/sym/KAT-2499".to_string(),
                    stderr: String::new(),
                }),
                ("gh", ["pr", "view", "--json", "url,state,headRefName,baseRefName"]) => Ok(CommandOutcome {
                    success: true,
                    stdout: r#"{"url":"https://github.com/gannonh/kata/pull/999","state":"CLOSED","headRefName":"sym/KAT-2499","baseRefName":"main"}"#.to_string(),
                    stderr: String::new(),
                }),
                _ => panic!("unexpected command: {} {:?}", program, args),
            }
            },
        );

        assert_eq!(
            status,
            AgentReviewPrStatus::Missing {
                branch: Some("sym/KAT-2499".to_string()),
                reason: "PR exists but is not open (state: CLOSED)".to_string(),
            }
        );
    }

    #[test]
    fn agent_review_pr_status_reports_missing_remote_branch() {
        let workspace = Path::new("/tmp/workspace");
        let status = check_agent_review_pr_status_with(
            workspace,
            Some("main"),
            |program, args, _cwd| match (program, args) {
                ("git", ["branch", "--show-current"]) => Ok(CommandOutcome {
                    success: true,
                    stdout: "sym/KAT-2499".to_string(),
                    stderr: String::new(),
                }),
                ("git", ["ls-remote", "--exit-code", "--heads", "origin", "sym/KAT-2499"]) => {
                    Ok(CommandOutcome {
                        success: false,
                        stdout: String::new(),
                        stderr: String::new(),
                    })
                }
                _ => panic!("unexpected command: {} {:?}", program, args),
            },
        );

        assert_eq!(
            status,
            AgentReviewPrStatus::Missing {
                branch: Some("sym/KAT-2499".to_string()),
                reason: "remote branch is missing on origin (ref not found)".to_string(),
            }
        );
    }

    #[test]
    fn agent_review_pr_status_rejects_head_ref_mismatch() {
        let workspace = Path::new("/tmp/workspace");
        let status = check_agent_review_pr_status_with(
            workspace,
            Some("main"),
            |program, args, _cwd| {
                match (program, args) {
                ("git", ["branch", "--show-current"]) => Ok(CommandOutcome {
                    success: true,
                    stdout: "sym/KAT-2499".to_string(),
                    stderr: String::new(),
                }),
                ("git", ["ls-remote", "--exit-code", "--heads", "origin", "sym/KAT-2499"]) => {
                    Ok(CommandOutcome {
                        success: true,
                        stdout: "abc123\trefs/heads/sym/KAT-2499".to_string(),
                        stderr: String::new(),
                    })
                }
                ("gh", ["pr", "view", "--json", "url,state,headRefName,baseRefName"]) => {
                    Ok(CommandOutcome {
                        success: true,
                        stdout: r#"{"url":"https://github.com/gannonh/kata/pull/999","state":"OPEN","headRefName":"sym/KAT-9999","baseRefName":"main"}"#.to_string(),
                        stderr: String::new(),
                    })
                }
                _ => panic!("unexpected command: {} {:?}", program, args),
            }
            },
        );

        assert_eq!(
            status,
            AgentReviewPrStatus::Missing {
                branch: Some("sym/KAT-2499".to_string()),
                reason:
                    "PR head branch `sym/KAT-9999` does not match current branch `sym/KAT-2499`"
                        .to_string(),
            }
        );
    }

    #[test]
    fn agent_review_pr_status_rejects_base_ref_mismatch() {
        let workspace = Path::new("/tmp/workspace");
        let status = check_agent_review_pr_status_with(
            workspace,
            Some("main"),
            |program, args, _cwd| {
                match (program, args) {
                ("git", ["branch", "--show-current"]) => Ok(CommandOutcome {
                    success: true,
                    stdout: "sym/KAT-2499".to_string(),
                    stderr: String::new(),
                }),
                ("git", ["ls-remote", "--exit-code", "--heads", "origin", "sym/KAT-2499"]) => {
                    Ok(CommandOutcome {
                        success: true,
                        stdout: "abc123\trefs/heads/sym/KAT-2499".to_string(),
                        stderr: String::new(),
                    })
                }
                ("gh", ["pr", "view", "--json", "url,state,headRefName,baseRefName"]) => {
                    Ok(CommandOutcome {
                        success: true,
                        stdout: r#"{"url":"https://github.com/gannonh/kata/pull/999","state":"OPEN","headRefName":"sym/KAT-2499","baseRefName":"develop"}"#.to_string(),
                        stderr: String::new(),
                    })
                }
                _ => panic!("unexpected command: {} {:?}", program, args),
            }
            },
        );

        assert_eq!(
            status,
            AgentReviewPrStatus::Missing {
                branch: Some("sym/KAT-2499".to_string()),
                reason: "PR base branch `develop` does not match expected `main`".to_string(),
            }
        );
    }

    #[test]
    fn agent_review_pr_status_check_failed_on_command_error() {
        let workspace = Path::new("/tmp/workspace");
        let status = check_agent_review_pr_status_with(
            workspace,
            Some("main"),
            |program, args, _cwd| match (program, args) {
                ("git", ["branch", "--show-current"]) => {
                    Err("git branch --show-current timed out after 30s".to_string())
                }
                _ => panic!("unexpected command: {} {:?}", program, args),
            },
        );

        assert_eq!(
            status,
            AgentReviewPrStatus::CheckFailed {
                reason: "could not determine current branch: git branch --show-current timed out after 30s".to_string(),
            }
        );
    }
}
