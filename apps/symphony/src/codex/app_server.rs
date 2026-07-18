//! Codex app-server client — subprocess lifecycle, handshake, and turn streaming.
//!
//! Ports the Elixir `SymphonyElixir.Codex.AppServer` module to idiomatic Rust.
//!
//! ## Protocol overview
//!
//! 1. Validate workspace cwd against the workspace root.
//! 2. Spawn `bash -lc <codex.command>` with workspace as cwd.
//! 3. Handshake: `initialize(id=1)` → await response → `initialized` → `thread/start(id=2)` → await response.
//! 4. Turn: `turn/start(id=3)` → await response → stream events until `turn/completed` / failure / timeout.
//! 5. Stop: kill subprocess.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use chrono::Utc;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::codex::dynamic_tool;
use crate::codex::token_accounting::{extract_rate_limits, extract_token_delta, TokenState};
use crate::domain::{AgentEvent, CodexConfig, Issue};
use crate::error::{Result, SymphonyError};
use crate::path_safety;
use crate::ssh::SshRunner;

/// Answer sent to Codex when the session is non-interactive and cannot provide
/// operator input.  Matches Elixir's `@non_interactive_tool_input_answer`.
const NON_INTERACTIVE_ANSWER: &str =
    "This is a non-interactive session. Operator input is unavailable.";

// ── Constants ─────────────────────────────────────────────────────────

const INITIALIZE_ID: u64 = 1;
const THREAD_START_ID: u64 = 2;
const TURN_START_ID: u64 = 3;

/// Maximum bytes printed from non-JSON lines in logs.
const MAX_STREAM_LOG_BYTES: usize = 1_000;

/// Best-effort safe command label for diagnostics (avoids logging raw command lines).
fn command_label(command: &[String]) -> String {
    command
        .iter()
        .flat_map(|part| part.split_whitespace())
        .find(|token| !token.contains('='))
        .unwrap_or("configured-command")
        .to_string()
}

// ── Public types ──────────────────────────────────────────────────────

/// Opaque handle to a running Codex app-server subprocess session.
///
/// Holds the subprocess I/O channels and session state needed to run turns.
/// Created by `start_session`; consumed by `stop_session`.
pub struct SessionHandle {
    /// Session identifier.
    ///
    /// Initially set to the `thread_id` returned by the handshake.
    /// Updated to `"<thread_id>-<turn_id>"` format when `run_turn` starts.
    pub session_id: String,

    // ── Subprocess ────────────────────────────────────────────────────
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout_reader: BufReader<tokio::process::ChildStdout>,

    // ── Session state ─────────────────────────────────────────────────
    thread_id: String,
    /// OS PID of the subprocess as a string (for event metadata).
    pid: Option<String>,

    // ── Issue info (stored for turn/start and logging) ─────────────────
    issue_id: String,
    issue_identifier: String,
    issue_title: String,
    workspace_path: String,

    // ── Policy / timing ───────────────────────────────────────────────
    approval_policy: Value,
    turn_sandbox_policy: Option<Value>,
    turn_timeout_ms: u64,
    read_timeout_ms: u64,
    /// Whether approval requests from Codex should be auto-approved.
    ///
    /// Set to `true` when `approval_policy == "never"` (i.e., the policy means
    /// "never require human approval" — auto-approve all requests). Mirrors
    /// Elixir's `auto_approve_requests: session_policies.approval_policy == "never"`.
    auto_approve_requests: bool,
}

/// The outcome of a completed Codex agent turn.
///
/// Contains the turn's event stream, final text, and token accounting.
#[derive(Debug)]
pub struct TurnResult {
    /// Events emitted during the turn, in delivery order.
    pub events: Vec<AgentEvent>,
    /// Final text output from the turn (if any).
    pub output_text: Option<String>,
    /// Incremental input tokens consumed during this turn.
    pub input_tokens: u64,
    /// Incremental output tokens consumed during this turn.
    pub output_tokens: u64,
    /// Incremental total tokens consumed during this turn (input + output + any other).
    pub total_tokens: u64,
    /// Rate-limit info captured from the last event that contained it.
    pub rate_limits: Option<Value>,
}

/// Optional Symphony helper environment passed through to worker subprocesses.
#[derive(Clone, Copy)]
pub struct HelperEnv<'a> {
    pub symphony_bin: Option<&'a str>,
    pub symphony_workflow_path: Option<&'a str>,
}

impl SessionHandle {
    /// Build a handle from parts already negotiated by the triage runner.
    ///
    /// Triage performs its own handshake (no dynamic tools, no helper env),
    /// then hands the session over so `run_turn` / `stop_session` can be reused.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_for_triage(
        child: tokio::process::Child,
        stdin: tokio::process::ChildStdin,
        stdout_reader: BufReader<tokio::process::ChildStdout>,
        thread_id: String,
        issue_id: String,
        issue_identifier: String,
        issue_title: String,
        workspace_path: String,
        config: &CodexConfig,
    ) -> Self {
        Self {
            session_id: thread_id.clone(),
            child,
            stdin,
            stdout_reader,
            thread_id,
            pid: None,
            issue_id,
            issue_identifier,
            issue_title,
            workspace_path,
            approval_policy: config.approval_policy.clone(),
            turn_sandbox_policy: config.turn_sandbox_policy.clone(),
            turn_timeout_ms: config.turn_timeout_ms,
            read_timeout_ms: config.read_timeout_ms,
            auto_approve_requests: false,
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────

/// Launch a Codex app-server subprocess and perform the startup handshake.
///
/// Validates the workspace path, spawns `bash -lc <command>`, performs the
/// JSON-RPC handshake (`initialize` → `initialized` → `thread/start`), and
/// returns a `SessionHandle` on success.
///
/// # Arguments
/// - `config`          — Codex runtime configuration
/// - `issue`           — issue being worked on (stored for turn/start and logging)
/// - `workspace_path`  — path to the workspace directory for this issue
/// - `workspace_root`  — workspace root used to validate containment
/// - `worker_host`     — if `Some(host)`, spawn via SSH on the remote host;
///   if `None`, spawn locally (default behaviour)
/// - `container_id`    — if `Some(id)`, spawn via Docker exec in the container
///
/// # Errors
/// - `InvalidWorkspaceCwd` — workspace path fails safety checks
/// - `CodexNotFound`       — bash or the configured command does not exist
/// - `SshLaunchFailed`     — SSH subprocess failed to start (remote path only)
/// - `ResponseTimeout`     — handshake did not complete in time
/// - `ResponseError`       — subprocess sent an unexpected response
pub async fn start_session(
    config: &CodexConfig,
    issue: &Issue,
    workspace_path: &Path,
    workspace_root: &Path,
    worker_host: Option<&str>,
    container_id: Option<&str>,
) -> Result<SessionHandle> {
    start_session_with_helper_env(
        config,
        issue,
        workspace_path,
        workspace_root,
        worker_host,
        container_id,
        HelperEnv {
            symphony_bin: None,
            symphony_workflow_path: None,
        },
    )
    .await
}

/// Start a Codex app-server session with Symphony helper environment injected.
#[allow(clippy::too_many_arguments)]
pub async fn start_session_with_helper_env(
    config: &CodexConfig,
    issue: &Issue,
    workspace_path: &Path,
    workspace_root: &Path,
    worker_host: Option<&str>,
    container_id: Option<&str>,
    helper_env: HelperEnv<'_>,
) -> Result<SessionHandle> {
    let cmd_str = config.command.join(" ");
    let cmd_label = command_label(&config.command);

    // ── Step 1 & 2: Validate + Spawn (local or remote) ───────────────
    let (workspace_str, mut child) = match (container_id, worker_host) {
        (Some(container_id), _) => {
            let workspace_str = workspace_path.to_string_lossy().to_string();

            tracing::info!(
                container_id = %container_id,
                issue_id = %issue.id,
                cmd = %cmd_str,
                "Spawning Codex via Docker exec"
            );

            let remote_cmd = format!(
                "cd {} && {}{}",
                crate::ssh::shell_escape(&workspace_str),
                shell_env_prefix(
                    helper_env.symphony_bin,
                    helper_env.symphony_workflow_path,
                    issue,
                    &workspace_str,
                ),
                cmd_str
            );
            let child = crate::docker::exec_command(container_id, &remote_cmd)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| SymphonyError::DockerContainerFailed(e.to_string()))?;

            (workspace_str, child)
        }
        (None, None) => {
            // ── Local path (unchanged behaviour) ─────────────────────
            let canonical_workspace = validate_workspace_cwd(workspace_path, workspace_root)?;
            let workspace_str = canonical_workspace.to_string_lossy().to_string();

            tracing::debug!(
                issue_id = %issue.id,
                cmd = %cmd_str,
                cwd = %workspace_str,
                "Spawning Codex app-server"
            );

            let mut command = tokio::process::Command::new("bash");
            command
                .args(["-lc", &cmd_str])
                .current_dir(&canonical_workspace)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            command.env("SYMPHONY_ISSUE_ID", &issue.id);
            command.env("SYMPHONY_ISSUE_IDENTIFIER", &issue.identifier);
            command.env("SYMPHONY_ISSUE_TITLE", &issue.title);
            command.env("SYMPHONY_WORKSPACE_PATH", &workspace_str);
            if let Some(symphony_bin) = helper_env.symphony_bin {
                command.env("SYMPHONY_BIN", symphony_bin);
            }
            if let Some(symphony_workflow_path) = helper_env.symphony_workflow_path {
                command.env("SYMPHONY_WORKFLOW_PATH", symphony_workflow_path);
            }
            let child = command.spawn().map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    SymphonyError::CodexNotFound
                } else {
                    SymphonyError::Io(e)
                }
            })?;

            (workspace_str, child)
        }
        (None, Some(host)) => {
            // ── Remote path via SSH ───────────────────────────────────
            let workspace_str =
                crate::ssh::validate_remote_workspace_cwd(&workspace_path.to_string_lossy())?;

            tracing::info!(
                worker_host = %host,
                issue_id = %issue.id,
                cmd = %cmd_str,
                "Spawning remote Codex via SSH"
            );

            // Prepend `cd <workspace> &&` so the remote shell starts in the
            // workspace directory — matching the local path's `.current_dir()`.
            let remote_cmd = format!(
                "cd {} && {}{}",
                crate::ssh::shell_escape(&workspace_str),
                shell_env_prefix(
                    helper_env.symphony_bin,
                    helper_env.symphony_workflow_path,
                    issue,
                    &workspace_str,
                ),
                cmd_str
            );
            let child = SshRunner::start_process(host, &remote_cmd).await?;
            (workspace_str, child)
        }
    };

    let pid = child.id().map(|p| p.to_string());
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    // Spawn a fire-and-forget task that logs stderr output
    tokio::spawn(drain_stderr(stderr));

    let mut stdout_reader = BufReader::new(stdout);

    // ── Step 3: Perform startup handshake ────────────────────────────
    let thread_id =
        match do_start_session(&mut stdin, &mut stdout_reader, config, &workspace_str).await {
            Ok(id) => id,
            Err(err) => {
                // Kill the subprocess before propagating the error
                let _ = child.kill().await;

                let enriched_err = match err {
                    SymphonyError::ResponseError(message) => {
                        SymphonyError::ResponseError(format!("{message}; command `{cmd_label}`"))
                    }
                    SymphonyError::ResponseTimeout => SymphonyError::ResponseError(format!(
                        "codex response timeout while starting command `{cmd_label}`"
                    )),
                    other => other,
                };

                return Err(enriched_err);
            }
        };

    // `auto_approve_requests` mirrors Elixir:
    // `auto_approve_requests: session_policies.approval_policy == "never"`
    let auto_approve_requests = config.approval_policy == Value::String("never".to_string());

    Ok(SessionHandle {
        session_id: thread_id.clone(),
        child,
        stdin,
        stdout_reader,
        thread_id,
        pid,
        issue_id: issue.id.clone(),
        issue_identifier: issue.identifier.clone(),
        issue_title: issue.title.clone(),
        workspace_path: workspace_str,
        approval_policy: config.approval_policy.clone(),
        turn_sandbox_policy: config.turn_sandbox_policy.clone(),
        turn_timeout_ms: config.turn_timeout_ms,
        read_timeout_ms: config.read_timeout_ms,
        auto_approve_requests,
    })
}

fn shell_env_prefix(
    symphony_bin: Option<&str>,
    symphony_workflow_path: Option<&str>,
    issue: &Issue,
    workspace_path: &str,
) -> String {
    let mut assignments = vec![
        format!("SYMPHONY_ISSUE_ID={}", crate::ssh::shell_escape(&issue.id)),
        format!(
            "SYMPHONY_ISSUE_IDENTIFIER={}",
            crate::ssh::shell_escape(&issue.identifier)
        ),
        format!(
            "SYMPHONY_ISSUE_TITLE={}",
            crate::ssh::shell_escape(&issue.title)
        ),
        format!(
            "SYMPHONY_WORKSPACE_PATH={}",
            crate::ssh::shell_escape(workspace_path)
        ),
    ];
    if let Some(symphony_bin) = symphony_bin {
        assignments.push(format!(
            "SYMPHONY_BIN={}",
            crate::ssh::shell_escape(symphony_bin)
        ));
    }
    if let Some(symphony_workflow_path) = symphony_workflow_path {
        assignments.push(format!(
            "SYMPHONY_WORKFLOW_PATH={}",
            crate::ssh::shell_escape(symphony_workflow_path)
        ));
    }
    format!("{} ", assignments.join(" "))
}

/// Run a single agent turn and stream events via the provided callback.
///
/// Sends `turn/start`, streams line-delimited JSON until the turn completes,
/// fails, or times out. Emits `AgentEvent` variants via `event_callback` for
/// every lifecycle event.
///
/// # Arguments
/// - `handle`           — session handle from `start_session`
/// - `prompt`           — text prompt to send as turn input
/// - `graphql_executor` — injectable async function for `linear_graphql` tool calls.
///   Called as `graphql_executor(query, variables)`.
///   Clone-able so it can be invoked once per tool call.
/// - `event_callback`   — called for each `AgentEvent` as it arrives
///
/// # Errors
/// - `TurnFailed`       — turn ended with a `turn/failed` message
/// - `TurnCancelled`    — turn was cancelled (`turn/cancelled` message)
/// - `TurnInputRequired`— Codex requested operator input in a non-interactive session
/// - `TurnTimeout`      — no event received within `turn_timeout_ms`
/// - `PortExit`         — subprocess exited unexpectedly
pub async fn run_turn<E, EFut>(
    handle: &mut SessionHandle,
    prompt: &str,
    graphql_executor: E,
    mut event_callback: impl FnMut(AgentEvent) + Send,
) -> Result<TurnResult>
where
    E: Fn(String, Value) -> EFut + Clone + Send,
    EFut: Future<Output = crate::error::Result<Value>> + Send,
{
    // ── Send turn/start (id=3) ────────────────────────────────────────
    let title = format!("{}: {}", handle.issue_identifier, handle.issue_title);
    let turn_start_msg = json!({
        "method": "turn/start",
        "id": TURN_START_ID,
        "params": {
            "threadId": handle.thread_id,
            "input": [{"type": "text", "text": prompt}],
            "cwd": handle.workspace_path,
            "title": title,
            "approvalPolicy": handle.approval_policy,
            "sandboxPolicy": handle.turn_sandbox_policy
        }
    });
    send_message(&mut handle.stdin, &turn_start_msg).await?;

    // ── Await turn/start response, extract turn_id ────────────────────
    let turn_result = await_response(
        &mut handle.stdout_reader,
        TURN_START_ID,
        handle.read_timeout_ms,
    )
    .await?;

    let turn_id = turn_result
        .get("turn")
        .and_then(|t| t.get("id"))
        .and_then(|id| id.as_str())
        .ok_or_else(|| {
            SymphonyError::ResponseError(format!(
                "turn/start response missing turn.id: {:?}",
                turn_result
            ))
        })?
        .to_string();

    // ── Update session_id and emit SessionStarted ─────────────────────
    let session_id = format!("{}-{}", handle.thread_id, turn_id);
    handle.session_id = session_id.clone();

    tracing::info!(
        issue_id = %handle.issue_id,
        issue_identifier = %handle.issue_identifier,
        session_id = %session_id,
        "Codex session started"
    );

    let session_started = AgentEvent::SessionStarted {
        timestamp: Utc::now(),
        codex_app_server_pid: handle.pid.clone(),
        session_id: session_id.clone(),
    };
    event_callback(session_started.clone());

    let mut events = vec![session_started];

    // ── Turn-local token state ────────────────────────────────────────
    let mut token_state = TokenState::default();
    let mut turn_input_tokens: u64 = 0;
    let mut turn_output_tokens: u64 = 0;
    let mut turn_total_tokens: u64 = 0;
    let mut turn_rate_limits: Option<Value> = None;

    // ── Receive loop ──────────────────────────────────────────────────
    loop {
        let mut line = String::new();

        let read_result = tokio::time::timeout(
            Duration::from_millis(handle.turn_timeout_ms),
            handle.stdout_reader.read_line(&mut line),
        )
        .await;

        match read_result {
            // ── Timeout ──────────────────────────────────────────────
            Err(_elapsed) => {
                tracing::warn!(
                    issue_id = %handle.issue_id,
                    session_id = %session_id,
                    turn_timeout_ms = handle.turn_timeout_ms,
                    "Codex turn timed out"
                );
                return Err(SymphonyError::TurnTimeout);
            }

            // ── I/O error ─────────────────────────────────────────────
            Ok(Err(e)) => return Err(SymphonyError::Io(e)),

            // ── EOF — subprocess exited ───────────────────────────────
            Ok(Ok(0)) => {
                let status = tokio::time::timeout(Duration::from_secs(5), handle.child.wait())
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .and_then(|s| s.code())
                    .unwrap_or(-1);

                tracing::warn!(
                    issue_id = %handle.issue_id,
                    session_id = %session_id,
                    exit_status = status,
                    "Codex subprocess exited during turn"
                );

                return Err(SymphonyError::PortExit(status));
            }

            // ── Got a line ────────────────────────────────────────────
            Ok(Ok(_n)) => {
                let text = line.trim_end_matches(['\n', '\r']).to_string();
                if text.is_empty() {
                    continue;
                }

                match serde_json::from_str::<Value>(&text) {
                    // ── Valid JSON ────────────────────────────────────
                    Ok(payload) => {
                        let method = payload
                            .get("method")
                            .and_then(|m| m.as_str())
                            .map(|s| s.to_string());

                        tracing::debug!(
                            issue_id = %handle.issue_id,
                            method = method.as_deref().unwrap_or("(none)"),
                            "codex message received"
                        );

                        // ── Token accounting ──────────────────────────
                        let delta = extract_token_delta(&token_state, &payload);
                        turn_input_tokens += delta.input_tokens;
                        turn_output_tokens += delta.output_tokens;
                        turn_total_tokens += delta.total_tokens;
                        token_state.last_input = delta.input_reported;
                        token_state.last_output = delta.output_reported;
                        token_state.last_total = delta.total_reported;
                        if let Some(rl) = extract_rate_limits(&payload) {
                            turn_rate_limits = Some(rl);
                        }

                        match method.as_deref() {
                            // ── turn/completed ────────────────────────
                            Some("turn/completed") => {
                                // Check if turn/completed carries a failure status
                                // (e.g. usageLimitExceeded, model unavailable)
                                let turn_status = payload
                                    .pointer("/params/turn/status")
                                    .and_then(|s| s.as_str())
                                    .unwrap_or("completed");

                                if turn_status == "failed" {
                                    let error_msg = payload
                                        .pointer("/params/turn/error/message")
                                        .and_then(|m| m.as_str())
                                        .unwrap_or("turn completed with failed status");
                                    let error_code = payload
                                        .pointer("/params/turn/error/codexErrorInfo")
                                        .map(|c| {
                                            c.as_str()
                                                .map(String::from)
                                                .unwrap_or_else(|| c.to_string())
                                        })
                                        .unwrap_or_else(|| "unknown".to_string());

                                    tracing::warn!(
                                        issue_id = %handle.issue_id,
                                        session_id = %session_id,
                                        error_code = %error_code,
                                        error = %error_msg,
                                        "Codex turn failed"
                                    );

                                    let event = AgentEvent::TurnFailed {
                                        timestamp: Utc::now(),
                                        codex_app_server_pid: handle.pid.clone(),
                                        turn_id: turn_id.clone(),
                                        error: format!("{error_code}: {error_msg}"),
                                    };
                                    event_callback(event.clone());
                                    events.push(event);

                                    return Err(SymphonyError::TurnFailed(format!(
                                        "{error_code}: {error_msg}"
                                    )));
                                }

                                let event = AgentEvent::TurnCompleted {
                                    timestamp: Utc::now(),
                                    codex_app_server_pid: handle.pid.clone(),
                                    turn_id: turn_id.clone(),
                                    message: None,
                                    input_tokens: turn_input_tokens,
                                    output_tokens: turn_output_tokens,
                                    total_tokens: turn_total_tokens,
                                    rate_limits: turn_rate_limits.clone(),
                                };
                                event_callback(event.clone());
                                events.push(event);

                                tracing::info!(
                                    issue_id = %handle.issue_id,
                                    issue_identifier = %handle.issue_identifier,
                                    session_id = %session_id,
                                    total_tokens = turn_total_tokens,
                                    "Codex session completed"
                                );

                                return Ok(TurnResult {
                                    events,
                                    output_text: None,
                                    input_tokens: turn_input_tokens,
                                    output_tokens: turn_output_tokens,
                                    total_tokens: turn_total_tokens,
                                    rate_limits: turn_rate_limits,
                                });
                            }

                            // ── turn/failed ───────────────────────────
                            Some("turn/failed") => {
                                let params = payload.get("params").cloned().unwrap_or(Value::Null);
                                let error_msg = serde_json::to_string(&params)
                                    .unwrap_or_else(|_| format!("{params:?}"));

                                let event = AgentEvent::TurnFailed {
                                    timestamp: Utc::now(),
                                    codex_app_server_pid: handle.pid.clone(),
                                    turn_id: turn_id.clone(),
                                    error: error_msg.clone(),
                                };
                                event_callback(event.clone());
                                events.push(event);

                                tracing::warn!(
                                    issue_id = %handle.issue_id,
                                    session_id = %session_id,
                                    error = %error_msg,
                                    "Codex turn failed"
                                );

                                return Err(SymphonyError::TurnFailed(error_msg));
                            }

                            // ── turn/cancelled ────────────────────────
                            Some("turn/cancelled") => {
                                let params = payload.get("params").cloned().unwrap_or(Value::Null);
                                let reason = serde_json::to_string(&params)
                                    .unwrap_or_else(|_| format!("{params:?}"));

                                let event = AgentEvent::TurnCancelled {
                                    timestamp: Utc::now(),
                                    codex_app_server_pid: handle.pid.clone(),
                                    turn_id: turn_id.clone(),
                                };
                                event_callback(event.clone());
                                events.push(event);

                                tracing::warn!(
                                    issue_id = %handle.issue_id,
                                    session_id = %session_id,
                                    "Codex turn cancelled"
                                );

                                return Err(SymphonyError::TurnCancelled(reason));
                            }

                            // ── Approval: item/commandExecution/requestApproval ──
                            Some(m @ "item/commandExecution/requestApproval") => {
                                let mut ctx = ApprovalCtx {
                                    stdin: &mut handle.stdin,
                                    event_callback: &mut event_callback,
                                    events: &mut events,
                                    pid: handle.pid.clone(),
                                };
                                if !handle_approval_or_reject(
                                    &mut ctx,
                                    &payload,
                                    m,
                                    "acceptForSession",
                                    handle.auto_approve_requests,
                                )
                                .await?
                                {
                                    return Err(SymphonyError::Other(
                                        "approval_required".to_string(),
                                    ));
                                }
                            }

                            // ── Approval: execCommandApproval ─────────
                            Some(m @ "execCommandApproval") => {
                                let mut ctx = ApprovalCtx {
                                    stdin: &mut handle.stdin,
                                    event_callback: &mut event_callback,
                                    events: &mut events,
                                    pid: handle.pid.clone(),
                                };
                                if !handle_approval_or_reject(
                                    &mut ctx,
                                    &payload,
                                    m,
                                    "approved_for_session",
                                    handle.auto_approve_requests,
                                )
                                .await?
                                {
                                    return Err(SymphonyError::Other(
                                        "approval_required".to_string(),
                                    ));
                                }
                            }

                            // ── Approval: applyPatchApproval ──────────
                            Some(m @ "applyPatchApproval") => {
                                let mut ctx = ApprovalCtx {
                                    stdin: &mut handle.stdin,
                                    event_callback: &mut event_callback,
                                    events: &mut events,
                                    pid: handle.pid.clone(),
                                };
                                if !handle_approval_or_reject(
                                    &mut ctx,
                                    &payload,
                                    m,
                                    "approved_for_session",
                                    handle.auto_approve_requests,
                                )
                                .await?
                                {
                                    return Err(SymphonyError::Other(
                                        "approval_required".to_string(),
                                    ));
                                }
                            }

                            // ── Approval: item/fileChange/requestApproval ─
                            Some(m @ "item/fileChange/requestApproval") => {
                                let mut ctx = ApprovalCtx {
                                    stdin: &mut handle.stdin,
                                    event_callback: &mut event_callback,
                                    events: &mut events,
                                    pid: handle.pid.clone(),
                                };
                                if !handle_approval_or_reject(
                                    &mut ctx,
                                    &payload,
                                    m,
                                    "acceptForSession",
                                    handle.auto_approve_requests,
                                )
                                .await?
                                {
                                    return Err(SymphonyError::Other(
                                        "approval_required".to_string(),
                                    ));
                                }
                            }

                            // ── Tool call: item/tool/call ─────────────
                            Some("item/tool/call") => {
                                dispatch_tool_call(
                                    &mut handle.stdin,
                                    &payload,
                                    graphql_executor.clone(),
                                    handle.pid.clone(),
                                    &mut event_callback,
                                    &mut events,
                                )
                                .await?;
                                // Tool calls do NOT terminate the turn — continue loop
                            }

                            // ── User input: item/tool/requestUserInput ──
                            Some("item/tool/requestUserInput") => {
                                if !handle_request_user_input(
                                    &mut handle.stdin,
                                    &payload,
                                    handle.auto_approve_requests,
                                    handle.pid.clone(),
                                    &mut event_callback,
                                    &mut events,
                                )
                                .await?
                                {
                                    // Hard failure: questions present but IDs not extractable
                                    let event = AgentEvent::TurnInputRequired {
                                        timestamp: Utc::now(),
                                        codex_app_server_pid: handle.pid.clone(),
                                        turn_id: turn_id.clone(),
                                        prompt: Some(
                                            text.chars().take(MAX_STREAM_LOG_BYTES).collect(),
                                        ),
                                    };
                                    event_callback(event.clone());
                                    events.push(event);
                                    tracing::warn!(
                                        issue_id = %handle.issue_id,
                                        session_id = %session_id,
                                        "Codex turn requires unhandleable input"
                                    );
                                    return Err(SymphonyError::TurnInputRequired);
                                }
                            }

                            // ── Other / notification ──────────────────
                            Some(other_method) => {
                                // Detect generic needs_input patterns
                                if needs_input(other_method, &payload) {
                                    tracing::warn!(
                                        issue_id = %handle.issue_id,
                                        session_id = %session_id,
                                        method = %other_method,
                                        "Codex turn requires operator input"
                                    );
                                    let event = AgentEvent::TurnInputRequired {
                                        timestamp: Utc::now(),
                                        codex_app_server_pid: handle.pid.clone(),
                                        turn_id: turn_id.clone(),
                                        prompt: Some(
                                            text.chars().take(MAX_STREAM_LOG_BYTES).collect(),
                                        ),
                                    };
                                    event_callback(event.clone());
                                    events.push(event);
                                    return Err(SymphonyError::TurnInputRequired);
                                }

                                tracing::debug!("Codex notification: {:?}", other_method);
                                let event = AgentEvent::Notification {
                                    timestamp: Utc::now(),
                                    codex_app_server_pid: handle.pid.clone(),
                                    message: text.chars().take(MAX_STREAM_LOG_BYTES).collect(),
                                };
                                event_callback(event.clone());
                                events.push(event);
                            }

                            // ── JSON without method ───────────────────
                            None => {
                                let event = AgentEvent::OtherMessage {
                                    timestamp: Utc::now(),
                                    codex_app_server_pid: handle.pid.clone(),
                                    raw: payload,
                                };
                                event_callback(event.clone());
                                events.push(event);
                            }
                        }
                    }

                    // ── Non-JSON line ─────────────────────────────────
                    Err(parse_err) => {
                        log_non_json_line(&text, "turn stream");
                        let event = AgentEvent::Malformed {
                            timestamp: Utc::now(),
                            codex_app_server_pid: handle.pid.clone(),
                            raw_text: text,
                            parse_error: parse_err.to_string(),
                        };
                        event_callback(event.clone());
                        events.push(event);
                    }
                }
            }
        }
    }
}

/// Gracefully stop a Codex app-server session.
///
/// Closes stdin (signals EOF to the subprocess), kills it if still running,
/// and waits for it to exit. Consumes the `SessionHandle`.
pub async fn stop_session(mut handle: SessionHandle) -> Result<()> {
    // Drop stdin to signal EOF
    drop(handle.stdin);
    // Kill in case the process ignores the EOF
    let _ = handle.child.kill().await;
    let _ = handle.child.wait().await;
    Ok(())
}

// ── Approval / tool-call / user-input handlers ───────────────────────

/// Bundled context for approval handling to stay within the 7-argument limit.
struct ApprovalCtx<'a, CB: FnMut(AgentEvent) + Send> {
    stdin: &'a mut tokio::process::ChildStdin,
    event_callback: &'a mut CB,
    events: &'a mut Vec<AgentEvent>,
    pid: Option<String>,
}

/// Handle an approval request from Codex.
///
/// When `auto_approve` is `true`:
/// - Sends `{"id": <id>, "result": {"decision": "<decision>"}}` back to Codex.
/// - Emits `ApprovalAutoApproved` and returns `Ok(true)` (caller: continue loop).
///
/// When `auto_approve` is `false`:
/// - Emits `ApprovalRequired` and returns `Ok(false)` (caller: return error).
///
/// Returns `Err` only on I/O failure while writing the response.
async fn handle_approval_or_reject<CB: FnMut(AgentEvent) + Send>(
    ctx: &mut ApprovalCtx<'_, CB>,
    payload: &Value,
    method: &str,
    decision: &str,
    auto_approve: bool,
) -> Result<bool> {
    let stdin = &mut ctx.stdin;
    let event_callback = &mut ctx.event_callback;
    let events = &mut ctx.events;
    let pid = ctx.pid.clone();
    if auto_approve {
        let id = payload.get("id").cloned().unwrap_or(Value::Null);
        let response = json!({
            "id": id,
            "result": {"decision": decision}
        });
        send_message(stdin, &response).await?;

        let event = AgentEvent::ApprovalAutoApproved {
            timestamp: Utc::now(),
            codex_app_server_pid: pid,
            tool_call: method.to_string(),
        };
        event_callback(event.clone());
        events.push(event);

        tracing::debug!(method = %method, decision = %decision, "Approval auto-approved");
        Ok(true)
    } else {
        let event = AgentEvent::ApprovalRequired {
            timestamp: Utc::now(),
            codex_app_server_pid: pid,
            method: method.to_string(),
            payload: payload.clone(),
        };
        event_callback(event.clone());
        events.push(event);

        tracing::warn!(method = %method, "Approval required — returning error");
        Ok(false)
    }
}

/// Dispatch an `item/tool/call` payload to `dynamic_tool::execute`.
///
/// - Extracts `tool_name` and `arguments` from `params` (lenient, nil if blank).
/// - Calls `dynamic_tool::execute(name, arguments, executor)`.
/// - Normalizes the result (ensures `output` and `contentItems`).
/// - Sends `{"id": <id>, "result": <result>}` back to Codex.
/// - Emits `ToolCallCompleted`, `ToolCallFailed`, or `UnsupportedToolCall`.
/// - Returns `Ok(())` — tool calls never terminate the turn.
async fn dispatch_tool_call<E, EFut>(
    stdin: &mut tokio::process::ChildStdin,
    payload: &Value,
    graphql_executor: E,
    pid: Option<String>,
    event_callback: &mut (impl FnMut(AgentEvent) + Send),
    events: &mut Vec<AgentEvent>,
) -> Result<()>
where
    E: Fn(String, Value) -> EFut,
    EFut: Future<Output = crate::error::Result<Value>>,
{
    let id = payload.get("id").cloned().unwrap_or(Value::Null);
    let params = payload.get("params").cloned().unwrap_or(Value::Null);

    let tool_name = extract_tool_name(&params);
    let arguments = extract_tool_arguments(&params);

    let result = dynamic_tool::execute(
        tool_name.as_deref().unwrap_or(""),
        arguments,
        graphql_executor,
    )
    .await;

    let normalized = normalize_tool_result(&result);

    // Send result back to Codex
    let response = json!({
        "id": id,
        "result": normalized
    });
    send_message(stdin, &response).await?;

    // Emit event based on outcome
    let event = if result.success {
        AgentEvent::ToolCallCompleted {
            timestamp: Utc::now(),
            codex_app_server_pid: pid,
            tool_name: tool_name.clone().unwrap_or_default(),
        }
    } else if tool_name.is_none() {
        AgentEvent::UnsupportedToolCall {
            timestamp: Utc::now(),
            codex_app_server_pid: pid,
            tool_name: String::new(),
        }
    } else {
        AgentEvent::ToolCallFailed {
            timestamp: Utc::now(),
            codex_app_server_pid: pid,
            tool_name,
        }
    };

    event_callback(event.clone());
    events.push(event);

    Ok(())
}

/// Handle an `item/tool/requestUserInput` payload.
///
/// Returns:
/// - `Ok(true)` — answered and loop should continue
/// - `Ok(false)` — hard failure, no IDs extractable (caller must return error)
///
/// Behaviour depends on `auto_approve` and question structure:
/// 1. `auto_approve=true` AND all questions have approval options →
///    sends option-based answer, emits `ApprovalAutoApproved`.
/// 2. Otherwise → sends non-interactive answer per question ID,
///    emits `ToolInputAutoAnswered`.
/// 3. Question IDs not extractable → returns `Ok(false)`.
async fn handle_request_user_input(
    stdin: &mut tokio::process::ChildStdin,
    payload: &Value,
    auto_approve: bool,
    pid: Option<String>,
    event_callback: &mut (impl FnMut(AgentEvent) + Send),
    events: &mut Vec<AgentEvent>,
) -> Result<bool> {
    let id = payload.get("id").cloned().unwrap_or(Value::Null);
    let params = payload.get("params").cloned().unwrap_or(Value::Null);

    // Attempt 1: option-based approval auto-answer (only when auto_approve=true)
    if auto_approve {
        if let Some((answers, decision)) = build_approval_answers(&params) {
            let response = json!({
                "id": id,
                "result": {"answers": answers}
            });
            send_message(stdin, &response).await?;

            let event = AgentEvent::ApprovalAutoApproved {
                timestamp: Utc::now(),
                codex_app_server_pid: pid,
                tool_call: decision,
            };
            event_callback(event.clone());
            events.push(event);

            tracing::debug!("User-input approval auto-answered via option selection");
            return Ok(true);
        }
    }

    // Attempt 2: non-interactive answer per question ID
    match build_non_interactive_answers(&params) {
        Some(answers) => {
            let response = json!({
                "id": id,
                "result": {"answers": answers}
            });
            send_message(stdin, &response).await?;

            let event = AgentEvent::ToolInputAutoAnswered {
                timestamp: Utc::now(),
                codex_app_server_pid: pid,
            };
            event_callback(event.clone());
            events.push(event);

            tracing::debug!("User-input answered with non-interactive response");
            Ok(true)
        }
        None => {
            // Cannot extract question IDs → hard failure
            tracing::warn!("Cannot answer user input: question IDs not extractable");
            Ok(false)
        }
    }
}

// ── Tool-call helpers ─────────────────────────────────────────────────

/// Extract the tool name from `item/tool/call` params.
///
/// Checks `params.tool`, then `params.name`.  Trims whitespace; returns `None`
/// if missing or blank — matching Elixir's `tool_call_name/1`.
fn extract_tool_name(params: &Value) -> Option<String> {
    let name = params
        .get("tool")
        .or_else(|| params.get("name"))
        .and_then(|v| v.as_str())?;
    let trimmed = name.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Extract tool arguments from `item/tool/call` params.
///
/// Returns `params.arguments` if present, or an empty object.
/// Mirrors Elixir's `tool_call_arguments/1`.
fn extract_tool_arguments(params: &Value) -> Value {
    params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}))
}

/// Normalize a `ToolResult` to the Codex wire format.
///
/// Ensures `success`, `output`, and `contentItems` are always present,
/// matching Elixir's `normalize_dynamic_tool_result/1`.
fn normalize_tool_result(result: &dynamic_tool::ToolResult) -> Value {
    json!({
        "success": result.success,
        "output": result.output,
        "contentItems": result.content_items
    })
}

// ── User-input helpers ────────────────────────────────────────────────

/// Try to build option-based approval answers for all questions.
///
/// Returns `Some((answers_map, decision_label))` if every question has an
/// approval option.  Returns `None` if any question lacks one.
///
/// Preference order: "Approve this Session" > "Approve Once" > any label
/// starting with "approve" or "allow" (case-insensitive).
fn build_approval_answers(params: &Value) -> Option<(Value, String)> {
    let questions = params.get("questions")?.as_array()?;
    if questions.is_empty() {
        return None;
    }

    let mut answers = serde_json::Map::new();
    for question in questions {
        let question_id = question.get("id")?.as_str()?;
        let options = question.get("options")?.as_array()?;
        let label = find_approval_option_label(options)?;
        answers.insert(question_id.to_string(), json!({"answers": [label]}));
    }

    Some((Value::Object(answers), "Approve this Session".to_string()))
}

/// Find the best approval option label from a list of options.
///
/// Preference: "Approve this Session" > "Approve Once" > starts with "approve"/"allow".
fn find_approval_option_label(options: &[Value]) -> Option<String> {
    let labels: Vec<&str> = options
        .iter()
        .filter_map(|opt| opt.get("label")?.as_str())
        .collect();

    // Preference 1: exact "Approve this Session"
    if let Some(&l) = labels.iter().find(|&&l| l == "Approve this Session") {
        return Some(l.to_string());
    }
    // Preference 2: exact "Approve Once"
    if let Some(&l) = labels.iter().find(|&&l| l == "Approve Once") {
        return Some(l.to_string());
    }
    // Preference 3: starts with "approve" or "allow" (case-insensitive)
    labels
        .iter()
        .find(|&&l| {
            let lower = l.trim().to_ascii_lowercase();
            lower.starts_with("approve") || lower.starts_with("allow")
        })
        .map(|&l| l.to_string())
}

/// Build non-interactive answers for all questions.
///
/// Returns `Some(answers_map)` if every question has a string `id` field.
/// Returns `None` if any question lacks one.
fn build_non_interactive_answers(params: &Value) -> Option<Value> {
    let questions = params.get("questions")?.as_array()?;
    if questions.is_empty() {
        return None;
    }

    let mut answers = serde_json::Map::new();
    for question in questions {
        let question_id = question.get("id")?.as_str()?;
        answers.insert(
            question_id.to_string(),
            json!({"answers": [NON_INTERACTIVE_ANSWER]}),
        );
    }

    Some(Value::Object(answers))
}

// ── needs_input detection ─────────────────────────────────────────────

/// Detect whether `method` + `payload` indicate a generic input-required event.
///
/// Ports Elixir's `needs_input?/2`:
/// - Method must start with `"turn/"`.
/// - Method is in the known input-required list, OR payload/params have a
///   `requiresInput`, `needsInput`, `input_required`, `inputRequired`, `type`
///   flag set to `true` / `"input_required"` / `"needs_input"`.
fn needs_input(method: &str, payload: &Value) -> bool {
    if !method.starts_with("turn/") {
        return false;
    }
    is_input_required_method(method, payload)
}

fn is_input_required_method(method: &str, payload: &Value) -> bool {
    const INPUT_METHODS: &[&str] = &[
        "turn/input_required",
        "turn/needs_input",
        "turn/need_input",
        "turn/request_input",
        "turn/request_response",
        "turn/provide_input",
        "turn/approval_required",
    ];

    if INPUT_METHODS.contains(&method) {
        return true;
    }

    // Check payload and params for input-required flags
    let params = payload.get("params");
    payload_needs_input(payload) || params.map(payload_needs_input).unwrap_or(false)
}

fn payload_needs_input(payload: &Value) -> bool {
    let obj = match payload.as_object() {
        Some(o) => o,
        None => return false,
    };

    obj.get("requiresInput").and_then(|v| v.as_bool()) == Some(true)
        || obj.get("needsInput").and_then(|v| v.as_bool()) == Some(true)
        || obj.get("input_required").and_then(|v| v.as_bool()) == Some(true)
        || obj.get("inputRequired").and_then(|v| v.as_bool()) == Some(true)
        || obj.get("type").and_then(|v| v.as_str()) == Some("input_required")
        || obj.get("type").and_then(|v| v.as_str()) == Some("needs_input")
}

// ── Workspace cwd validation ──────────────────────────────────────────

/// Validate that `workspace_path` is a legitimate subdirectory of `workspace_root`.
///
/// Rejects:
/// - workspace == root (identical canonical paths)
/// - workspace outside root (canonical path not under root)
/// - symlink escape (canonical path outside root, but non-resolved path appears inside)
///
/// Returns the canonicalized workspace path on success.
fn validate_workspace_cwd(workspace_path: &Path, workspace_root: &Path) -> Result<PathBuf> {
    let canonical_workspace = path_safety::canonicalize(workspace_path)?;
    let canonical_root = path_safety::canonicalize(workspace_root)?;

    // Expand without symlink resolution for escape detection
    let expanded_workspace = expand_path_no_symlinks(workspace_path);
    let expanded_root = expand_path_no_symlinks(workspace_root);

    // ── Check 1: workspace IS the root ───────────────────────────────
    if canonical_workspace == canonical_root {
        return Err(SymphonyError::InvalidWorkspaceCwd(format!(
            "workspace is the workspace root: {}",
            canonical_workspace.display()
        )));
    }

    // ── Check 2: canonical path is properly under root ────────────────
    // `Path::starts_with` checks component boundaries, so /tmp/abc does NOT
    // match /tmp/a, but /tmp/a/b does match /tmp/a.
    if canonical_workspace.starts_with(&canonical_root) {
        return Ok(canonical_workspace);
    }

    // ── Check 3: symlink escape ───────────────────────────────────────
    // The non-resolved path is under root, but the canonical path is not.
    // This means a symlink inside the root points outside it.
    if expanded_workspace.starts_with(&expanded_root) && expanded_workspace != expanded_root {
        return Err(SymphonyError::InvalidWorkspaceCwd(format!(
            "symlink escape: {} resolves outside workspace root {}",
            workspace_path.display(),
            canonical_root.display()
        )));
    }

    // ── Check 4: completely outside root ─────────────────────────────
    Err(SymphonyError::InvalidWorkspaceCwd(format!(
        "workspace {} is outside root {}",
        canonical_workspace.display(),
        canonical_root.display()
    )))
}

/// Return the absolute, `.`/`..`-normalized path WITHOUT following symlinks.
fn expand_path_no_symlinks(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };

    let mut normalized = PathBuf::new();
    for component in abs.components() {
        match component {
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::CurDir => {}
            c => normalized.push(c),
        }
    }
    normalized
}

// ── I/O helpers ───────────────────────────────────────────────────────

/// JSON-encode `message`, append a newline, and write to subprocess stdin.
async fn send_message(stdin: &mut tokio::process::ChildStdin, message: &Value) -> Result<()> {
    let encoded = serde_json::to_string(message)
        .map_err(|e| SymphonyError::Other(format!("json_encode_failed: {e}")))?;
    let line = format!("{encoded}\n");
    stdin.write_all(line.as_bytes()).await?;
    stdin.flush().await?;
    Ok(())
}

/// Read lines from `reader` until a JSON response with `request_id` is found.
///
/// Each `read_line` call is wrapped in `tokio::time::timeout(timeout_ms)`.
/// Non-matching JSON messages are skipped (logged at DEBUG).
/// Non-JSON lines are logged and skipped.
/// Returns the `result` field of the matching response on success.
async fn await_response(
    reader: &mut BufReader<tokio::process::ChildStdout>,
    request_id: u64,
    timeout_ms: u64,
) -> Result<Value> {
    loop {
        let mut line = String::new();

        let read_result = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            reader.read_line(&mut line),
        )
        .await;

        match read_result {
            Err(_elapsed) => return Err(SymphonyError::ResponseTimeout),
            Ok(Err(e)) => return Err(SymphonyError::Io(e)),
            Ok(Ok(0)) => {
                return Err(SymphonyError::ResponseError(
                    "subprocess exited during handshake".to_string(),
                ));
            }
            Ok(Ok(_n)) => {
                let text = line.trim_end_matches(['\n', '\r']).to_string();
                if text.is_empty() {
                    continue;
                }

                match serde_json::from_str::<Value>(&text) {
                    Ok(payload) => {
                        let id = payload.get("id").and_then(|v| v.as_u64());

                        if id == Some(request_id) {
                            // Found the response we were waiting for
                            if let Some(error) = payload.get("error") {
                                return Err(SymphonyError::ResponseError(
                                    serde_json::to_string(error)
                                        .unwrap_or_else(|_| format!("{error:?}")),
                                ));
                            }
                            if let Some(result) = payload.get("result") {
                                return Ok(result.clone());
                            }
                            return Err(SymphonyError::ResponseError(format!(
                                "response for id={request_id} has no result field: {text}"
                            )));
                        }

                        // Different ID — ignore and keep reading
                        tracing::debug!(
                            "Ignoring message while waiting for response id={}: {}",
                            request_id,
                            text.chars().take(200).collect::<String>()
                        );
                    }
                    Err(_) => {
                        // Non-JSON — log and keep reading
                        log_non_json_line(&text, "response stream");
                    }
                }
            }
        }
    }
}

// ── Handshake ─────────────────────────────────────────────────────────

/// Perform the full startup handshake and return the `thread_id`.
///
/// Order: `initialize(id=1)` → await → `initialized` (no id) → `thread/start(id=2)` → await.
async fn do_start_session(
    stdin: &mut tokio::process::ChildStdin,
    reader: &mut BufReader<tokio::process::ChildStdout>,
    config: &CodexConfig,
    workspace_path: &str,
) -> Result<String> {
    // ── Send initialize (id=1) ────────────────────────────────────────
    let init_msg = json!({
        "method": "initialize",
        "id": INITIALIZE_ID,
        "params": {
            "capabilities": {
                "experimentalApi": true
            },
            "clientInfo": {
                "name": "symphony-orchestrator",
                "title": "Symphony Orchestrator",
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    });
    send_message(stdin, &init_msg).await?;

    // ── Await response to initialize ──────────────────────────────────
    await_response(reader, INITIALIZE_ID, config.read_timeout_ms).await?;

    // ── Send initialized notification (no id) ─────────────────────────
    let initialized_msg = json!({
        "method": "initialized",
        "params": {}
    });
    send_message(stdin, &initialized_msg).await?;

    // ── Send thread/start (id=2) ──────────────────────────────────────
    let thread_start_msg = json!({
        "method": "thread/start",
        "id": THREAD_START_ID,
        "params": {
            "approvalPolicy": config.approval_policy,
            "sandbox": config.thread_sandbox,
            "cwd": workspace_path,
            "dynamicTools": dynamic_tool::tool_specs()
        }
    });
    send_message(stdin, &thread_start_msg).await?;

    // ── Await response to thread/start, extract thread_id ────────────
    let thread_result = await_response(reader, THREAD_START_ID, config.read_timeout_ms).await?;

    let thread_id = thread_result
        .get("thread")
        .and_then(|t| t.get("id"))
        .and_then(|id| id.as_str())
        .ok_or_else(|| {
            SymphonyError::ResponseError(format!("invalid thread payload: {:?}", thread_result))
        })?
        .to_string();

    Ok(thread_id)
}

// ── Observability helpers ─────────────────────────────────────────────

/// Log a non-JSON stream line at WARN (if it looks like an error) or DEBUG.
fn log_non_json_line(text: &str, stream_label: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    let truncated = if trimmed.len() > MAX_STREAM_LOG_BYTES {
        // Walk back to a valid UTF-8 char boundary to avoid a panic on
        // multi-byte characters (e.g. Unicode identifiers, emoji in code).
        let mut end = MAX_STREAM_LOG_BYTES;
        while end > 0 && !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        &trimmed[..end]
    } else {
        trimmed
    };

    if looks_like_error(truncated) {
        tracing::warn!("Codex {} output: {}", stream_label, truncated);
    } else {
        tracing::debug!("Codex {} output: {}", stream_label, truncated);
    }
}

/// Return true if `text` looks like an error/warning line.
fn looks_like_error(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("error")
        || lower.contains("warning")
        || lower.contains("failed")
        || lower.contains("fatal")
        || lower.contains("panic")
        || lower.contains("exception")
}

/// Read all lines from the subprocess stderr and log them at DEBUG.
/// Runs as a fire-and-forget tokio task.
async fn drain_stderr(stderr: tokio::process::ChildStderr) {
    use tokio::io::AsyncBufReadExt as _;
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    tracing::debug!(target: "codex_stderr", "{}", trimmed);
                }
            }
        }
    }
}
