use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::{timeout, Instant};

use crate::domain::{CodexConfig, Issue};
use crate::error::{Result, SymphonyError};
use crate::triage::artifact::{self, ArtifactValidationError};
use crate::triage::domain::{StageUsage, TriageArtifact};
use crate::triage::integrity::{self, RepoBaseline};
use crate::triage::process_identity::{self, ProcessIdentity};

const FORCE_KILL_WAIT: Duration = Duration::from_secs(5);
const OUTPUT_ENV: &str = "SYMPHONY_STAGE_OUTPUT";
const MODEL_ENV: &str = "SYMPHONY_TRIAGE_MODEL";
/// Bytes of child stdout/stderr retained for failure diagnostics.
const CHILD_OUTPUT_TAIL: usize = 4_000;

/// Harness executing the triage turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriageHarness {
    Pi,
    Codex,
}

/// Pi model precedence: triage.model, then agent.model, then harness default (None).
pub fn effective_pi_model(triage_model: Option<&str>, agent_model: Option<&str>) -> Option<String> {
    triage_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            agent_model
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

#[derive(Clone)]
pub struct TriageRunnerRequest {
    pub attempt_id: String,
    pub workspace_root: PathBuf,
    pub repo_path: PathBuf,
    pub command: Vec<String>,
    /// Rendered triage prompt (template already expanded by the coordinator).
    pub prompt: String,
    pub turn_timeout_ms: u64,
    /// Effective Pi model. Codex callers pass `None` (app-server default).
    pub model: Option<String>,
    pub harness: TriageHarness,
    /// Issue identity used for Codex session metadata (no env injection).
    pub issue: TriageIssueIdentity,
    /// Codex policy settings, when `harness == Codex`.
    pub codex: Option<CodexConfig>,
    /// Live progress sink; events arrive as the turn executes.
    pub progress: Option<TriageProgressSink>,
    /// Notified once the child is running, so the attempt's OS identity and
    /// disposable paths are durable before the turn can be interrupted.
    pub spawned: Option<TriageSpawnSink>,
}

/// Callback receiving structured progress during a triage turn.
pub type TriageProgressSink = std::sync::Arc<dyn Fn(TriageProgress) + Send + Sync>;

/// Callback receiving the spawned child's identity, fired once per turn.
pub type TriageSpawnSink = std::sync::Arc<dyn Fn(TriageSpawnInfo) + Send + Sync>;

/// What a restart needs to identify and clean up an abandoned attempt.
#[derive(Debug, Clone)]
pub struct TriageSpawnInfo {
    pub identity: ProcessIdentity,
    pub workspace_path: PathBuf,
    pub output_path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct TriageProgress {
    /// Last notable event name (e.g. `tool_execution_start`, `turn_end`).
    pub last_event: Option<String>,
    /// Human-readable summary of the last event.
    pub message: Option<String>,
    /// Tool currently executing (cleared on tool end).
    pub current_tool_name: Option<String>,
    /// Short preview of the current tool's arguments.
    pub current_tool_args_preview: Option<String>,
    /// Session identifier when known (pi session id or codex thread-turn).
    pub session_id: Option<String>,
    /// Total tokens when the harness reports them.
    pub total_tokens: Option<u64>,
}

impl std::fmt::Debug for TriageRunnerRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TriageRunnerRequest")
            .field("attempt_id", &self.attempt_id)
            .field("workspace_root", &self.workspace_root)
            .field("repo_path", &self.repo_path)
            .field("command", &self.command)
            .field("turn_timeout_ms", &self.turn_timeout_ms)
            .field("model", &self.model)
            .field("harness", &self.harness)
            .field("issue", &self.issue)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct TriageIssueIdentity {
    pub id: String,
    pub identifier: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct TriageRunnerSuccess {
    pub artifact: TriageArtifact,
    pub usage: StageUsage,
    pub workspace_path: PathBuf,
    pub output_path: PathBuf,
    pub baseline: RepoBaseline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriageRunnerFailureKind {
    Setup,
    Spawn,
    Timeout,
    NonZeroExit,
    MissingOutput,
    InvalidArtifact,
    Integrity,
}

#[derive(Debug, Clone)]
pub struct TriageRunnerFailure {
    pub kind: TriageRunnerFailureKind,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum TriageRunnerOutcome {
    Success(Box<TriageRunnerSuccess>),
    Failure(TriageRunnerFailure),
}

struct AttemptLayout {
    attempt_root: PathBuf,
    workspace_path: PathBuf,
    output_path: PathBuf,
    home_dir: PathBuf,
}

pub struct TriageRunner;

impl TriageRunner {
    pub async fn run(request: TriageRunnerRequest) -> Result<TriageRunnerOutcome> {
        if let Err(message) = validate_request(&request) {
            return Ok(failure(TriageRunnerFailureKind::Setup, message));
        }

        let layout = match prepare_attempt(&request) {
            Ok(layout) => layout,
            Err(outcome) => return Ok(outcome),
        };

        let baseline = match integrity::capture_baseline(&layout.workspace_path) {
            Ok(baseline) => baseline,
            Err(err) => {
                let _ = fs::remove_dir_all(&layout.attempt_root);
                return Ok(failure(TriageRunnerFailureKind::Setup, err.to_string()));
            }
        };

        let execution = match request.harness {
            TriageHarness::Pi => run_pi_turn(&request, &layout).await,
            TriageHarness::Codex => run_codex_turn(&request, &layout).await,
        };

        let usage = match execution {
            Ok(usage) => usage,
            Err(outcome) => {
                let _ = fs::remove_dir_all(&layout.attempt_root);
                return Ok(outcome);
            }
        };

        finish_attempt(&layout, baseline, usage)
    }
}

fn validate_request(request: &TriageRunnerRequest) -> std::result::Result<(), String> {
    if request.command.is_empty() {
        return Err("triage command cannot be empty".to_string());
    }
    if request.prompt.trim().is_empty() {
        return Err("triage prompt cannot be empty".to_string());
    }
    if request.turn_timeout_ms == 0 {
        return Err("triage turn_timeout_ms must be greater than zero".to_string());
    }
    if !request.repo_path.is_dir() {
        return Err(format!(
            "triage repo path does not exist: {}",
            request.repo_path.display()
        ));
    }
    Ok(())
}

fn prepare_attempt(
    request: &TriageRunnerRequest,
) -> std::result::Result<AttemptLayout, TriageRunnerOutcome> {
    let attempt_root = request
        .workspace_root
        .join(format!("triage-{}", request.attempt_id));
    let layout = AttemptLayout {
        workspace_path: attempt_root.join("workspace"),
        output_path: attempt_root.join("stage-output").join("result.json"),
        home_dir: attempt_root.join("home"),
        attempt_root,
    };

    if let Err(err) = prepare_dirs(
        &layout.workspace_path,
        layout.output_path.parent().expect("output dir"),
        &layout.home_dir,
    ) {
        return Err(failure(TriageRunnerFailureKind::Setup, err.to_string()));
    }

    if let Err(err) = clone_local_workspace(&request.repo_path, &layout.workspace_path) {
        let _ = fs::remove_dir_all(&layout.attempt_root);
        return Err(failure(TriageRunnerFailureKind::Setup, err));
    }

    if let Err(err) = disable_push_urls(&layout.workspace_path) {
        let _ = fs::remove_dir_all(&layout.attempt_root);
        return Err(failure(TriageRunnerFailureKind::Setup, err));
    }

    Ok(layout)
}

fn finish_attempt(
    layout: &AttemptLayout,
    baseline: RepoBaseline,
    usage: StageUsage,
) -> Result<TriageRunnerOutcome> {
    let bytes = match fs::read(&layout.output_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            let _ = fs::remove_dir_all(&layout.attempt_root);
            return Ok(failure(
                TriageRunnerFailureKind::MissingOutput,
                format!(
                    "missing triage output at {}: {err}",
                    layout.output_path.display()
                ),
            ));
        }
    };

    let artifact = match artifact::parse_and_validate(&bytes) {
        Ok(artifact) => artifact,
        Err(err) => {
            let _ = fs::remove_dir_all(&layout.attempt_root);
            return Ok(failure(
                TriageRunnerFailureKind::InvalidArtifact,
                format_artifact_error(err),
            ));
        }
    };

    if let Err(err) = integrity::check_repository_integrity(&layout.workspace_path, &baseline) {
        let _ = fs::remove_dir_all(&layout.attempt_root);
        return Ok(failure(TriageRunnerFailureKind::Integrity, err.to_string()));
    }

    // Drop copied provider credentials before retaining successful evidence.
    // Fail closed: do not report success if the isolated home cannot be removed.
    if let Err(err) = scrub_isolated_home(&layout.home_dir) {
        let _ = fs::remove_dir_all(&layout.attempt_root);
        return Ok(failure(
            TriageRunnerFailureKind::Setup,
            format!(
                "failed to scrub isolated triage home {}: {err}",
                layout.home_dir.display()
            ),
        ));
    }

    Ok(TriageRunnerOutcome::Success(Box::new(
        TriageRunnerSuccess {
            artifact,
            usage,
            workspace_path: layout.workspace_path.clone(),
            output_path: layout.output_path.clone(),
            baseline,
        },
    )))
}

/// Remove the isolated home (and any seeded credentials) after the child exits.
/// Publish the running child's identity and disposable paths to the caller.
///
/// Recovery after a crash depends entirely on this record, so it is emitted as
/// soon as the child exists rather than when the turn completes.
fn report_spawn(request: &TriageRunnerRequest, layout: &AttemptLayout, child_id: Option<u32>) {
    let (Some(sink), Some(pid)) = (request.spawned.as_ref(), child_id) else {
        return;
    };
    sink(TriageSpawnInfo {
        identity: process_identity::capture(pid),
        workspace_path: layout.workspace_path.clone(),
        output_path: layout.output_path.clone(),
    });
}

fn scrub_isolated_home(home_dir: &Path) -> std::io::Result<()> {
    if home_dir.exists() {
        fs::remove_dir_all(home_dir)?;
    }
    Ok(())
}

// ── Pi one-shot print execution ─────────────────────────────────────────────

async fn run_pi_turn(
    request: &TriageRunnerRequest,
    layout: &AttemptLayout,
) -> std::result::Result<StageUsage, TriageRunnerOutcome> {
    let mut command_args = build_pi_command(&request.command, &request.prompt);
    if let Some(model) = request.model.as_deref() {
        if is_pi_like_command(&command_args) && !command_has_model_flag(&command_args) {
            command_args.push("--model".to_string());
            command_args.push(model.to_string());
        }
    }

    let env = build_isolated_env(
        &layout.home_dir,
        &layout.output_path,
        request.model.as_deref(),
    );
    seed_pi_auth(&layout.home_dir);
    let program = command_args[0].clone();

    let mut child_cmd = Command::new(&program);
    child_cmd
        .args(&command_args[1..])
        .current_dir(&layout.workspace_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(env);

    #[cfg(unix)]
    {
        child_cmd.process_group(0);
    }

    let mut child = match child_cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            return Err(failure(
                TriageRunnerFailureKind::Spawn,
                format!("failed to spawn triage command '{program}': {err}"),
            ));
        }
    };

    let child_id = child.id();
    report_spawn(request, layout, child_id);
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let turn_timeout = Duration::from_millis(request.turn_timeout_ms);

    // Stream pi --mode json events for live progress while retaining the raw
    // stream (bounded) for diagnostics.
    let progress = request.progress.clone();
    let mut stdout_reader = BufReader::new(stdout);
    let stdout_pump = tokio::spawn(async move {
        let mut raw: Vec<u8> = Vec::new();
        let mut line = String::new();
        loop {
            line.clear();
            match stdout_reader.read_line(&mut line).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if raw.len() < CHILD_OUTPUT_TAIL * 4 {
                        raw.extend_from_slice(line.as_bytes());
                    }
                    if let Some(sink) = &progress {
                        if let Some(event) = parse_pi_json_event(line.trim()) {
                            sink(event);
                        }
                    }
                }
            }
        }
        raw
    });
    let stderr_pump = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut raw: Vec<u8> = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match tokio::io::AsyncReadExt::read(&mut reader, &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if raw.len() < CHILD_OUTPUT_TAIL * 4 {
                        raw.extend_from_slice(&buf[..n]);
                    }
                }
            }
        }
        raw
    });

    let wait_result = timeout(turn_timeout, child.wait()).await;
    let status = match wait_result {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => {
            stdout_pump.abort();
            stderr_pump.abort();
            return Err(failure(
                TriageRunnerFailureKind::Spawn,
                format!("failed waiting for triage command: {err}"),
            ));
        }
        Err(_elapsed) => {
            terminate_process_group(child_id).await;
            stdout_pump.abort();
            stderr_pump.abort();
            return Err(failure(
                TriageRunnerFailureKind::Timeout,
                format!(
                    "triage turn exceeded turn_timeout_ms={}",
                    request.turn_timeout_ms
                ),
            ));
        }
    };

    let stdout_raw = stdout_pump.await.unwrap_or_default();
    let stderr_raw = stderr_pump.await.unwrap_or_default();

    if !status.success() {
        let code = status.code().unwrap_or(-1);
        let stderr_tail = tail(&stderr_raw, CHILD_OUTPUT_TAIL);
        let stdout_tail = tail(&stdout_raw, CHILD_OUTPUT_TAIL);
        return Err(failure(
            TriageRunnerFailureKind::NonZeroExit,
            format!(
                "triage command exited with status {code}; stderr: {stderr_tail}; stdout: {stdout_tail}"
            ),
        ));
    }

    Ok(StageUsage::default())
}

/// Map one `pi --mode json` line to a progress event.
fn parse_pi_json_event(line: &str) -> Option<TriageProgress> {
    if line.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let event_type = value.get("type")?.as_str()?;
    match event_type {
        "session" => Some(TriageProgress {
            last_event: Some("session".to_string()),
            message: Some("pi session started".to_string()),
            session_id: value
                .get("id")
                .and_then(|id| id.as_str())
                .map(str::to_string),
            ..Default::default()
        }),
        "tool_execution_start" => {
            let tool = value
                .get("toolName")
                .and_then(|name| name.as_str())
                .unwrap_or("tool");
            Some(TriageProgress {
                last_event: Some("tool_execution_start".to_string()),
                message: Some(format!("running {tool}")),
                current_tool_name: Some(tool.to_string()),
                current_tool_args_preview: value.get("args").map(|args| preview_json(args, 120)),
                ..Default::default()
            })
        }
        "tool_execution_end" => Some(TriageProgress {
            last_event: Some("tool_execution_end".to_string()),
            message: value
                .get("toolName")
                .and_then(|name| name.as_str())
                .map(|name| format!("finished {name}")),
            ..Default::default()
        }),
        "turn_end" => Some(TriageProgress {
            last_event: Some("turn_end".to_string()),
            message: Some("turn completed".to_string()),
            ..Default::default()
        }),
        "agent_end" => Some(TriageProgress {
            last_event: Some("agent_end".to_string()),
            message: Some("agent finished".to_string()),
            ..Default::default()
        }),
        _ => None,
    }
}

fn preview_json(value: &serde_json::Value, max: usize) -> String {
    let rendered = match value {
        serde_json::Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    let compact = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.len() <= max {
        compact
    } else {
        format!("{}…", &compact[..max])
    }
}

/// Build the pi one-shot invocation: strip `--mode <m>`, force `-p --no-session`,
/// use `--mode json` for a structured event stream, and append the rendered
/// prompt as the final positional argument.
fn build_pi_command(command: &[String], prompt: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut iter = command.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--mode" {
            let _ = iter.next();
            continue;
        }
        if arg.starts_with("--mode=") {
            continue;
        }
        if arg == "-p" || arg == "--print" || arg == "--no-session" {
            continue;
        }
        out.push(arg.clone());
    }
    out.push("-p".to_string());
    out.push("--no-session".to_string());
    out.push("--mode".to_string());
    out.push("json".to_string());
    out.push(prompt.to_string());
    out
}

fn tail(bytes: &[u8], max: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if trimmed.len() <= max {
        return trimmed.to_string();
    }
    let start = trimmed.len() - max;
    let boundary = trimmed
        .char_indices()
        .find(|(idx, _)| *idx >= start)
        .map(|(idx, _)| idx)
        .unwrap_or(trimmed.len());
    format!("...{}", &trimmed[boundary..])
}

// ── Codex app-server one-turn execution ─────────────────────────────────────

async fn run_codex_turn(
    request: &TriageRunnerRequest,
    layout: &AttemptLayout,
) -> std::result::Result<StageUsage, TriageRunnerOutcome> {
    let mut config = request.codex.clone().unwrap_or_default();
    config.command = request.command.clone();
    config.turn_timeout_ms = request.turn_timeout_ms;

    let issue = Issue {
        id: request.issue.id.clone(),
        identifier: request.issue.identifier.clone(),
        title: request.issue.title.clone(),
        description: None,
        priority: None,
        state: String::new(),
        branch_name: None,
        url: None,
        assignee_id: None,
        labels: Vec::new(),
        blocked_by: Vec::new(),
        assigned_to_worker: false,
        created_at: None,
        updated_at: None,
        children_count: 0,
        parent_identifier: None,
    };

    let handle = codex_session_start(request, &config, &issue, layout).await;
    let mut handle = match handle {
        Ok(handle) => handle,
        Err(outcome) => return Err(outcome),
    };

    let progress = request.progress.clone();
    let turn_result = crate::codex::app_server::run_turn(
        &mut handle,
        &request.prompt,
        |_query, _variables| async {
            Err(SymphonyError::TriageError(
                "dynamic tools are disabled for triage".to_string(),
            ))
        },
        move |event| {
            if let Some(sink) = &progress {
                if let Some(update) = codex_event_progress(&event) {
                    sink(update);
                }
            }
        },
    )
    .await;
    let _ = crate::codex::app_server::stop_session(handle).await;

    let result = match turn_result {
        Ok(result) => result,
        Err(err) => {
            return Err(failure(codex_failure_kind(&err), err.to_string()));
        }
    };

    if let Some(sink) = &request.progress {
        sink(TriageProgress {
            last_event: Some("turn_completed".to_string()),
            message: Some("codex turn completed".to_string()),
            total_tokens: Some(result.total_tokens),
            ..Default::default()
        });
    }

    Ok(StageUsage {
        input_tokens: result.input_tokens,
        output_tokens: result.output_tokens,
        total_tokens: result.total_tokens,
    })
}

fn codex_event_progress(event: &crate::domain::AgentEvent) -> Option<TriageProgress> {
    use crate::domain::AgentEvent;
    match event {
        AgentEvent::SessionStarted { session_id, .. } => Some(TriageProgress {
            last_event: Some("session_started".to_string()),
            message: Some("codex session started".to_string()),
            session_id: Some(session_id.clone()),
            ..Default::default()
        }),
        AgentEvent::ApprovalAutoApproved { tool_call, .. } => Some(TriageProgress {
            last_event: Some("tool_call".to_string()),
            message: Some(format!("approved {tool_call}")),
            current_tool_name: Some(tool_call.clone()),
            ..Default::default()
        }),
        AgentEvent::TurnCompleted {
            total_tokens,
            message,
            ..
        } => Some(TriageProgress {
            last_event: Some("turn_completed".to_string()),
            message: message
                .clone()
                .or_else(|| Some("turn completed".to_string())),
            total_tokens: Some(*total_tokens),
            ..Default::default()
        }),
        _ => None,
    }
}

fn codex_failure_kind(err: &SymphonyError) -> TriageRunnerFailureKind {
    match err {
        SymphonyError::TurnTimeout => TriageRunnerFailureKind::Timeout,
        _ => TriageRunnerFailureKind::NonZeroExit,
    }
}

/// Start a triage Codex session: no helper env, no `SYMPHONY_ISSUE_*`
/// injection, no dynamic tools, `SYMPHONY_STAGE_OUTPUT` set for the artifact.
///
/// Environment is cleared and rebuilt with an isolated `HOME` (same policy as
/// the Pi path) so operator credentials such as `GH_TOKEN` are not inherited.
async fn codex_session_start(
    request: &TriageRunnerRequest,
    config: &CodexConfig,
    issue: &Issue,
    layout: &AttemptLayout,
) -> std::result::Result<crate::codex::app_server::SessionHandle, TriageRunnerOutcome> {
    let workspace_str = layout.workspace_path.to_string_lossy().to_string();
    let cmd_str = config.command.join(" ");
    let env = build_isolated_env(&layout.home_dir, &layout.output_path, None);

    let mut command = Command::new("bash");
    command
        .args(["-lc", &cmd_str])
        .current_dir(&layout.workspace_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(env);

    let mut child = command.spawn().map_err(|err| {
        failure(
            TriageRunnerFailureKind::Spawn,
            format!("failed to spawn codex app-server: {err}"),
        )
    })?;

    report_spawn(request, layout, child.id());
    let mut stdin = child.stdin.take().expect("stdin piped");
    let stdout = child.stdout.take().expect("stdout piped");
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            while let Ok(n) = reader.read_line(&mut line).await {
                if n == 0 {
                    break;
                }
                line.clear();
            }
        });
    }
    let mut reader = BufReader::new(stdout);

    let thread_id =
        match triage_codex_handshake(&mut stdin, &mut reader, config, &workspace_str).await {
            Ok(id) => id,
            Err(err) => {
                let _ = child.kill().await;
                return Err(failure(
                    TriageRunnerFailureKind::Spawn,
                    format!("codex triage handshake failed: {err}"),
                ));
            }
        };

    Ok(crate::codex::app_server::SessionHandle::new_for_triage(
        child,
        stdin,
        reader,
        thread_id,
        issue.id.clone(),
        issue.identifier.clone(),
        issue.title.clone(),
        workspace_str,
        config,
    ))
}

/// Minimal initialize/initialized/thread/start handshake without dynamic tools.
async fn triage_codex_handshake(
    stdin: &mut tokio::process::ChildStdin,
    reader: &mut BufReader<tokio::process::ChildStdout>,
    config: &CodexConfig,
    workspace_str: &str,
) -> Result<String> {
    send_codex_message(
        stdin,
        &json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "capabilities": {"experimentalApi": true},
                "clientInfo": {
                    "name": "symphony-triage",
                    "title": "Symphony Triage",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        }),
    )
    .await?;
    await_codex_response(reader, 1, config.read_timeout_ms).await?;
    send_codex_message(stdin, &json!({"method": "initialized", "params": {}})).await?;
    send_codex_message(
        stdin,
        &json!({
            "method": "thread/start",
            "id": 2,
            "params": {
                "approvalPolicy": config.approval_policy,
                "sandbox": config.thread_sandbox,
                "cwd": workspace_str,
                "dynamicTools": []
            }
        }),
    )
    .await?;
    let thread_result = await_codex_response(reader, 2, config.read_timeout_ms).await?;
    thread_result
        .get("thread")
        .and_then(|thread| thread.get("id"))
        .and_then(|id| id.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            SymphonyError::ResponseError(format!("invalid thread/start payload: {thread_result:?}"))
        })
}

async fn send_codex_message(
    stdin: &mut tokio::process::ChildStdin,
    message: &serde_json::Value,
) -> Result<()> {
    let payload = serde_json::to_string(message)
        .map_err(|err| SymphonyError::ResponseError(err.to_string()))?;
    stdin.write_all(payload.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    Ok(())
}

async fn await_codex_response(
    reader: &mut BufReader<tokio::process::ChildStdout>,
    id: u64,
    timeout_ms: u64,
) -> Result<serde_json::Value> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(1_000));
    loop {
        let mut line = String::new();
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(SymphonyError::ResponseTimeout);
        }
        let read = timeout(remaining, reader.read_line(&mut line)).await;
        match read {
            Err(_) => return Err(SymphonyError::ResponseTimeout),
            Ok(Err(err)) => return Err(SymphonyError::Io(err)),
            Ok(Ok(0)) => {
                return Err(SymphonyError::ResponseError(
                    "codex app-server closed stdout during triage handshake".to_string(),
                ))
            }
            Ok(Ok(_)) => {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                    continue;
                };
                if value.get("id").and_then(|v| v.as_u64()) != Some(id) {
                    continue;
                }
                if let Some(error) = value.get("error") {
                    return Err(SymphonyError::ResponseError(error.to_string()));
                }
                return Ok(value
                    .get("result")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null));
            }
        }
    }
}

// ── Workspace helpers ───────────────────────────────────────────────────────

fn failure(kind: TriageRunnerFailureKind, message: impl Into<String>) -> TriageRunnerOutcome {
    TriageRunnerOutcome::Failure(TriageRunnerFailure {
        kind,
        message: message.into(),
    })
}

fn format_artifact_error(err: ArtifactValidationError) -> String {
    format!("invalid triage artifact: {err}")
}

fn prepare_dirs(workspace: &Path, output_dir: &Path, home_dir: &Path) -> std::io::Result<()> {
    if let Some(parent) = workspace.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir_all(workspace)?;
    fs::create_dir_all(output_dir)?;
    fs::create_dir_all(home_dir)?;
    Ok(())
}

fn clone_local_workspace(repo: &Path, workspace: &Path) -> std::result::Result<(), String> {
    let output = std::process::Command::new("git")
        .arg("clone")
        .arg("--local")
        .arg("--no-hardlinks")
        .arg(repo)
        .arg(".")
        .current_dir(workspace)
        .output()
        .map_err(|err| format!("git clone --local failed: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "git clone --local failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn disable_push_urls(workspace: &Path) -> std::result::Result<(), String> {
    let remotes = git_stdout(workspace, &["remote"])?;
    for remote in remotes
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let set = std::process::Command::new("git")
            .args(["remote", "set-url", "--push", remote, "DISABLED"])
            .current_dir(workspace)
            .output()
            .map_err(|err| format!("failed disabling push URL for {remote}: {err}"))?;
        if !set.status.success() {
            return Err(format!(
                "failed disabling push URL for {remote}: {}",
                String::from_utf8_lossy(&set.stderr).trim()
            ));
        }
    }
    Ok(())
}

fn git_stdout(workspace: &Path, args: &[&str]) -> std::result::Result<String, String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .map_err(|err| format!("git {} failed: {err}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn is_pi_like_command(command: &[String]) -> bool {
    command
        .first()
        .map(|program| {
            let name = Path::new(program)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(program);
            name == "pi" || name.starts_with("pi-")
        })
        .unwrap_or(false)
}

fn command_has_model_flag(command: &[String]) -> bool {
    command.iter().any(|arg| arg == "--model")
}

/// Copy file-backed pi provider auth into the isolated home.
///
/// OAuth providers (e.g. `openai-codex`) have no env-var credential path, so
/// without this the isolated harness cannot authenticate. Only auth material
/// is copied — never sessions, settings, or caches. Best-effort: missing
/// files are skipped (API-key env auth may still work).
fn seed_pi_auth(home_dir: &Path) {
    let Some(real_home) = std::env::var_os("HOME") else {
        return;
    };
    let source_dir = Path::new(&real_home).join(".pi").join("agent");
    let target_dir = home_dir.join(".pi").join("agent");
    for name in ["auth.json", "models.json"] {
        let source = source_dir.join(name);
        if !source.is_file() {
            continue;
        }
        if fs::create_dir_all(&target_dir).is_err() {
            return;
        }
        let _ = fs::copy(&source, target_dir.join(name));
    }
}

fn build_isolated_env(
    home_dir: &Path,
    output_path: &Path,
    model: Option<&str>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();
    let parent = std::env::vars().collect::<HashMap<_, _>>();

    if let Some(path) = parent.get("PATH") {
        env.insert("PATH".to_string(), path.clone());
    }
    if let Some(term) = parent.get("TERM") {
        env.insert("TERM".to_string(), term.clone());
    }

    for (key, value) in &parent {
        if key == "LANG" || key.starts_with("LC_") {
            env.insert(key.clone(), value.clone());
        }
        if matches!(key.as_str(), "TMPDIR" | "TMP" | "TEMP") {
            env.insert(key.clone(), value.clone());
        }
        if matches!(
            key.as_str(),
            "ANTHROPIC_API_KEY" | "OPENAI_API_KEY" | "CLAUDE_API_KEY"
        ) {
            env.insert(key.clone(), value.clone());
        }
    }

    env.insert("HOME".to_string(), home_dir.display().to_string());
    env.insert(OUTPUT_ENV.to_string(), output_path.display().to_string());
    if let Some(model) = model {
        env.insert(MODEL_ENV.to_string(), model.to_string());
    }
    env
}

async fn terminate_process_group(child_id: Option<u32>) {
    let Some(pid) = child_id else {
        return;
    };

    #[cfg(unix)]
    {
        unsafe {
            let _ = libc_kill(-(pid as i32), 15);
        }
        let deadline = Instant::now() + FORCE_KILL_WAIT;
        while Instant::now() < deadline {
            let alive = unsafe { libc_kill(-(pid as i32), 0) == 0 };
            if !alive {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        unsafe {
            let _ = libc_kill(-(pid as i32), 9);
            let _ = libc_kill(pid as i32, 9);
        }
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
    }
}

#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    kill(pid, sig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command as StdCommand;

    fn valid_artifact_json() -> String {
        serde_json::json!({
            "schema_version": 1,
            "route": "implement",
            "risk_class": "low",
            "rationale": "Bounded documentation fix.",
            "evidence": [{
                "kind": "issue",
                "reference": "body",
                "summary": "Issue names the file and replacement."
            }],
            "next_action": "Apply the documented replacement.",
            "clarification_question": null,
            "reproduction": null
        })
        .to_string()
    }

    fn init_repo(path: &Path) {
        fs::create_dir_all(path).unwrap();
        run(path, &["git", "init"]);
        run(path, &["git", "config", "user.email", "test@example.com"]);
        run(path, &["git", "config", "user.name", "Test"]);
        fs::write(path.join("README.md"), "hello\n").unwrap();
        fs::write(path.join(".gitignore"), "target/\n").unwrap();
        run(path, &["git", "add", "."]);
        run(path, &["git", "commit", "-m", "initial"]);
        run(
            path,
            &[
                "git",
                "remote",
                "add",
                "origin",
                "https://example.com/repo.git",
            ],
        );
    }

    fn run(path: &Path, args: &[&str]) {
        let output = StdCommand::new(args[0])
            .args(&args[1..])
            .current_dir(path)
            .output()
            .expect("command runs");
        assert!(
            output.status.success(),
            "{} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn write_script(path: &Path, body: &str) {
        fs::write(path, body).unwrap();
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }

    fn pi_request(repo: &Path, workspace_root: &Path, command: Vec<String>) -> TriageRunnerRequest {
        TriageRunnerRequest {
            attempt_id: "attempt-1".to_string(),
            workspace_root: workspace_root.to_path_buf(),
            repo_path: repo.to_path_buf(),
            command,
            prompt: "triage prompt".to_string(),
            turn_timeout_ms: 5_000,
            model: None,
            harness: TriageHarness::Pi,
            issue: TriageIssueIdentity {
                id: "1".to_string(),
                identifier: "#1".to_string(),
                title: "Fix docs".to_string(),
            },
            codex: None,
            progress: None,
            spawned: None,
        }
    }

    /// Restart recovery can only terminate an orphaned triage child if the
    /// running attempt published the child's identity and disposable paths
    /// while the turn was still in flight.
    #[tokio::test]
    async fn reports_spawned_child_identity_and_attempt_paths() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);
        let workspace_root = temp.path().join("workspaces");

        let script = temp.path().join("fake-pi.sh");
        write_script(
            &script,
            &format!(
                "#!/bin/sh\nprintf '%s' '{artifact}' > \"$SYMPHONY_STAGE_OUTPUT\"\n",
                artifact = valid_artifact_json().replace('\'', "'\"'\"'")
            ),
        );

        let seen: std::sync::Arc<std::sync::Mutex<Vec<TriageSpawnInfo>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        let mut request = pi_request(&repo, &workspace_root, vec![script.display().to_string()]);
        request.spawned = Some(std::sync::Arc::new(move |info: TriageSpawnInfo| {
            sink.lock().unwrap().push(info);
        }));

        let outcome = TriageRunner::run(request).await.unwrap();
        assert!(matches!(outcome, TriageRunnerOutcome::Success(_)));

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "exactly one spawn notification per turn");
        let info = &seen[0];
        assert!(info.identity.pid > 0);
        assert_eq!(
            info.identity.process_group_id, info.identity.pid,
            "child runs in its own process group"
        );
        assert!(
            info.workspace_path.starts_with(&workspace_root),
            "workspace path must locate the disposable clone"
        );
        assert!(info.output_path.ends_with("result.json"));
    }

    #[test]
    fn effective_pi_model_prefers_triage_then_agent() {
        assert_eq!(
            effective_pi_model(Some("triage-model"), Some("agent-model")).as_deref(),
            Some("triage-model")
        );
        assert_eq!(
            effective_pi_model(None, Some("agent-model")).as_deref(),
            Some("agent-model")
        );
        assert_eq!(
            effective_pi_model(Some("  "), Some("agent-model")).as_deref(),
            Some("agent-model")
        );
        assert_eq!(effective_pi_model(None, None), None);
    }

    #[test]
    fn build_pi_command_strips_mode_and_forces_print_no_session() {
        let command = vec![
            "pi".to_string(),
            "--mode".to_string(),
            "rpc".to_string(),
            "--thinking".to_string(),
            "low".to_string(),
        ];
        let built = build_pi_command(&command, "prompt text");
        assert_eq!(
            built,
            vec![
                "pi",
                "--thinking",
                "low",
                "-p",
                "--no-session",
                "--mode",
                "json",
                "prompt text"
            ]
        );
    }

    #[test]
    fn build_pi_command_dedupes_existing_print_flags() {
        let command = vec![
            "pi".to_string(),
            "-p".to_string(),
            "--no-session".to_string(),
        ];
        let built = build_pi_command(&command, "p");
        assert_eq!(
            built,
            vec!["pi", "-p", "--no-session", "--mode", "json", "p"]
        );
    }

    #[tokio::test]
    async fn successful_fake_runner_writes_artifact_and_omits_tokens() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);

        let script = temp.path().join("fake-runner.sh");
        write_script(
            &script,
            &format!(
                r#"#!/bin/sh
set -eu
echo "GH_TOKEN=${{GH_TOKEN-}}" > "$HOME/env-check.txt"
echo "GITHUB_TOKEN=${{GITHUB_TOKEN-}}" >> "$HOME/env-check.txt"
echo "SSH_AUTH_SOCK=${{SSH_AUTH_SOCK-}}" >> "$HOME/env-check.txt"
echo "SYMPHONY_BIN=${{SYMPHONY_BIN-}}" >> "$HOME/env-check.txt"
echo "MODEL=${{SYMPHONY_TRIAGE_MODEL-}}" >> "$HOME/env-check.txt"
printf '%s' '{artifact}' > "$SYMPHONY_STAGE_OUTPUT"
"#,
                artifact = valid_artifact_json().replace('\'', "'\"'\"'")
            ),
        );

        let mut request = pi_request(
            &repo,
            &temp.path().join("workspaces"),
            vec![script.display().to_string()],
        );
        request.model = Some("anthropic/claude-sonnet-4-6".to_string());

        let outcome = TriageRunner::run(request).await.unwrap();

        let success = match outcome {
            TriageRunnerOutcome::Success(success) => success,
            TriageRunnerOutcome::Failure(failure) => {
                panic!("expected success, got {:?}", failure);
            }
        };
        assert_eq!(success.artifact.route.as_str(), "implement");
        assert_eq!(success.usage.total_tokens, 0);

        // Isolated home (and any seeded credentials) must not be retained.
        let home_dir = success.workspace_path.parent().unwrap().join("home");
        assert!(
            !home_dir.exists(),
            "isolated home must be scrubbed after success"
        );
        assert!(success.output_path.is_file());

        let push = StdCommand::new("git")
            .args(["remote", "get-url", "--push", "origin"])
            .current_dir(&success.workspace_path)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&push.stdout).trim(), "DISABLED");
    }

    #[tokio::test]
    async fn successful_run_scrubs_seeded_pi_credentials() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);

        let real_home = temp.path().join("real-home");
        let agent_dir = real_home.join(".pi").join("agent");
        fs::create_dir_all(&agent_dir).unwrap();
        fs::write(agent_dir.join("auth.json"), r#"{"token":"secret"}"#).unwrap();
        fs::write(agent_dir.join("models.json"), r#"[]"#).unwrap();
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("HOME", &real_home);

        let script = temp.path().join("fake-runner.sh");
        write_script(
            &script,
            &format!(
                r#"#!/bin/sh
set -eu
test -f "$HOME/.pi/agent/auth.json"
printf '%s' '{artifact}' > "$SYMPHONY_STAGE_OUTPUT"
"#,
                artifact = valid_artifact_json().replace('\'', "'\"'\"'")
            ),
        );

        let request = pi_request(
            &repo,
            &temp.path().join("workspaces"),
            vec![script.display().to_string()],
        );
        let outcome = TriageRunner::run(request).await.unwrap();
        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }

        let success = match outcome {
            TriageRunnerOutcome::Success(success) => success,
            TriageRunnerOutcome::Failure(failure) => {
                panic!("expected success, got {:?}", failure);
            }
        };
        let home_dir = success.workspace_path.parent().unwrap().join("home");
        assert!(
            !home_dir.exists(),
            "seeded credentials home must be removed"
        );
        assert!(!home_dir.join(".pi/agent/auth.json").exists());
    }

    #[tokio::test]
    async fn timeout_kills_long_running_command() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);
        let script = temp.path().join("sleep.sh");
        write_script(&script, "#!/bin/sh\nsleep 30\n");

        let mut request = pi_request(
            &repo,
            &temp.path().join("workspaces"),
            vec![script.display().to_string()],
        );
        request.turn_timeout_ms = 200;

        let outcome = TriageRunner::run(request).await.unwrap();

        match outcome {
            TriageRunnerOutcome::Failure(failure) => {
                assert_eq!(failure.kind, TriageRunnerFailureKind::Timeout);
            }
            TriageRunnerOutcome::Success(_) => panic!("expected timeout"),
        }
    }

    #[tokio::test]
    async fn nonzero_exit_reports_stderr_tail() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);
        let script = temp.path().join("fail.sh");
        write_script(&script, "#!/bin/sh\necho 'something broke' >&2\nexit 3\n");

        let request = pi_request(
            &repo,
            &temp.path().join("workspaces"),
            vec![script.display().to_string()],
        );

        let outcome = TriageRunner::run(request).await.unwrap();

        match outcome {
            TriageRunnerOutcome::Failure(failure) => {
                assert_eq!(failure.kind, TriageRunnerFailureKind::NonZeroExit);
                assert!(failure.message.contains("status 3"));
                assert!(failure.message.contains("something broke"));
            }
            TriageRunnerOutcome::Success(_) => panic!("expected nonzero exit"),
        }
    }

    #[tokio::test]
    async fn integrity_failure_when_script_commits() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);
        let script = temp.path().join("commit.sh");
        write_script(
            &script,
            &format!(
                r#"#!/bin/sh
set -eu
echo dirty > dirty.rs
git add dirty.rs
git -c user.email=test@example.com -c user.name=Test commit -m dirty
printf '%s' '{artifact}' > "$SYMPHONY_STAGE_OUTPUT"
"#,
                artifact = valid_artifact_json().replace('\'', "'\"'\"'")
            ),
        );

        let request = pi_request(
            &repo,
            &temp.path().join("workspaces"),
            vec![script.display().to_string()],
        );

        let outcome = TriageRunner::run(request).await.unwrap();

        match outcome {
            TriageRunnerOutcome::Failure(failure) => {
                assert_eq!(failure.kind, TriageRunnerFailureKind::Integrity);
            }
            TriageRunnerOutcome::Success(_) => panic!("expected integrity failure"),
        }
    }

    #[tokio::test]
    async fn ignored_build_output_is_allowed() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);
        let script = temp.path().join("build.sh");
        write_script(
            &script,
            &format!(
                r#"#!/bin/sh
set -eu
mkdir -p target
echo generated > target/out.rs
printf '%s' '{artifact}' > "$SYMPHONY_STAGE_OUTPUT"
"#,
                artifact = valid_artifact_json().replace('\'', "'\"'\"'")
            ),
        );

        let request = pi_request(
            &repo,
            &temp.path().join("workspaces"),
            vec![script.display().to_string()],
        );

        let outcome = TriageRunner::run(request).await.unwrap();

        assert!(matches!(outcome, TriageRunnerOutcome::Success(_)));
    }

    #[test]
    fn seed_pi_auth_copies_auth_and_models_only() {
        let temp = tempfile::tempdir().unwrap();
        let fake_home = temp.path().join("realhome");
        let agent_dir = fake_home.join(".pi").join("agent");
        fs::create_dir_all(&agent_dir).unwrap();
        fs::write(agent_dir.join("auth.json"), "{}").unwrap();
        fs::write(agent_dir.join("models.json"), "{}").unwrap();
        fs::write(agent_dir.join("settings.json"), "{}").unwrap();

        let isolated = temp.path().join("isolated");
        let original = std::env::var_os("HOME");
        std::env::set_var("HOME", &fake_home);
        seed_pi_auth(&isolated);
        match original {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }

        let target = isolated.join(".pi").join("agent");
        assert!(target.join("auth.json").is_file());
        assert!(target.join("models.json").is_file());
        assert!(!target.join("settings.json").exists());
    }

    #[test]
    fn isolated_env_omits_forge_and_helper_vars() {
        std::env::set_var("GH_TOKEN", "secret");
        std::env::set_var("GITHUB_TOKEN", "secret");
        std::env::set_var("SSH_AUTH_SOCK", "/tmp/sock");
        std::env::set_var("GIT_ASKPASS", "askpass");
        std::env::set_var("SYMPHONY_BIN", "/usr/bin/symphony");
        std::env::set_var("SYMPHONY_WORKFLOW_PATH", "/tmp/workflow");
        std::env::set_var("ANTHROPIC_API_KEY", "provider");

        let env = build_isolated_env(
            Path::new("/tmp/home"),
            Path::new("/tmp/out.json"),
            Some("m"),
        );
        assert!(!env.contains_key("GH_TOKEN"));
        assert!(!env.contains_key("GITHUB_TOKEN"));
        assert!(!env.contains_key("SSH_AUTH_SOCK"));
        assert!(!env.contains_key("GIT_ASKPASS"));
        assert!(!env.contains_key("SYMPHONY_BIN"));
        assert!(!env.contains_key("SYMPHONY_WORKFLOW_PATH"));
        assert_eq!(
            env.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some("provider")
        );
        assert_eq!(
            env.get(OUTPUT_ENV).map(String::as_str),
            Some("/tmp/out.json")
        );
        assert_eq!(env.get(MODEL_ENV).map(String::as_str), Some("m"));
    }
}
