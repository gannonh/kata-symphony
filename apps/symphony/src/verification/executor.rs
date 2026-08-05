//! Stage-neutral command execution with pre-release launch barriers.
//!
//! Local execution spawns a new-process-group supervisor that blocks the
//! command payload on a controller-owned stdin pipe. Symphony durably
//! CAS-records the supervisor's PID/process-group/start-token/executable
//! identity before releasing the payload; pipe closure before release makes
//! the supervisor exit without running it. Docker execution uses labeled
//! `docker create` with the container ID durably recorded before `docker
//! start`, and recovery removes label-discoverable stopped orphans.
//!
//! Command and verifier child processes receive no forge, tracker, SSH,
//! helper, push, approval, merge, or deployment credentials.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::domain::DockerConfig;
use crate::error::{Result, SymphonyError};
use crate::implementation::validation::{bounded_redacted_tail, output_digest};
use crate::triage::process_identity::{self, ProcessIdentity, TerminationOutcome};

pub const FORBIDDEN_COMMAND_ENV: &[&str] = &[
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "LINEAR_API_KEY",
    "LINEAR_API_TOKEN",
    "SYMPHONY_HELPER_TOKEN",
    "SSH_AUTH_SOCK",
    "SSH_AGENT_PID",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
];

/// Environment variable carrying the trusted command into the supervisor.
const COMMAND_ENV: &str = "SYMPHONY_COMMAND";
/// Supervisor exit code when the release pipe closed before the payload ran.
const BARRIER_CLOSED_EXIT: i32 = 98;

/// Exit codes 0..=127 are real command exits; 128+ are signals (128+sig).
const SIGNAL_BASE: i32 = 128;

/// Output captured for one command run.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub output_sha256: String,
}

/// Result of one executed command.
#[derive(Debug, Clone)]
pub struct CommandExecutionResult {
    pub exit_code: Option<i32>,
    pub termination_reason: Option<String>,
    pub passed: bool,
    pub output: CommandOutput,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub duration_ms: u64,
}

/// Request for one command execution.
#[derive(Debug, Clone)]
pub struct CommandExecutionRequest {
    pub attempt_id: String,
    pub command_name: String,
    pub workspace_path: PathBuf,
    pub evidence_dir: PathBuf,
    pub home_dir: PathBuf,
    pub command: String,
    pub timeout_ms: u64,
    pub execution_profile: crate::implementation::domain::ExecutionProfile,
    pub docker: Option<DockerConfig>,
}

/// A failed command run always carries a concrete status.
#[derive(Debug, Clone)]
pub enum CommandRunFailure {
    /// The command exceeded its timeout; the process group (or container) was
    /// terminated and reaped. Partial captured output is attached.
    TimedOut(CommandOutput),
    SpawnError(String),
    NotSignalable(String),
    StillRunning(String),
}

impl std::fmt::Display for CommandRunFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut(_) => formatter.write_str("command exceeded its timeout"),
            Self::SpawnError(message) => write!(formatter, "spawn failed: {message}"),
            Self::NotSignalable(message) => formatter.write_str(message),
            Self::StillRunning(message) => formatter.write_str(message),
        }
    }
}

/// Execute one trusted command. `on_launch` runs once the child exists but
/// BEFORE the release barrier is lifted, and must durably CAS-record the
/// launch identity; its error aborts the launch without running the payload.
pub async fn execute_command(
    request: &CommandExecutionRequest,
    on_launch: impl FnOnce(LaunchIdentity) -> Result<()> + Send,
) -> std::result::Result<CommandExecutionResult, CommandRunFailure> {
    match request.execution_profile {
        crate::implementation::domain::ExecutionProfile::Local => {
            execute_local(request, on_launch).await
        }
        crate::implementation::domain::ExecutionProfile::Docker => {
            execute_docker(request, on_launch).await
        }
    }
}

/// Launch identity handed to the controller before the payload is released.
#[derive(Debug, Clone)]
pub enum LaunchIdentity {
    /// Local supervisor: PID, process group, start token, executable.
    Process(ProcessIdentity),
    /// Docker container created but not yet started.
    Container { container_id: String },
}

/// Assert helper for tests and doctor: the built environment carries none of
/// the forbidden credential keys.
pub fn assert_no_forge_credentials(env: &[(String, String)]) -> Result<()> {
    for (key, _) in env {
        if FORBIDDEN_COMMAND_ENV.iter().any(|forbidden| forbidden == key) {
            return Err(SymphonyError::TriageError(format!(
                "forbidden credential {key} present in command env"
            )));
        }
    }
    Ok(())
}

/// Build the minimal command environment. Only PATH/HOME and Symphony
/// workspace/evidence variables; never credentials.
pub fn command_env(
    home_dir: &Path,
    workspace_path: &Path,
    evidence_dir: &Path,
    attempt_id: &str,
    command: &str,
) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        env.push(("PATH".to_string(), path.to_string_lossy().to_string()));
    }
    env.push(("HOME".to_string(), home_dir.display().to_string()));
    env.push((
        "SYMPHONY_WORKSPACE".to_string(),
        workspace_path.display().to_string(),
    ));
    env.push((
        "SYMPHONY_EVIDENCE_DIR".to_string(),
        evidence_dir.display().to_string(),
    ));
    env.push(("SYMPHONY_ATTEMPT_ID".to_string(), attempt_id.to_string()));
    env.push((COMMAND_ENV.to_string(), command.to_string()));
    env
}

/// The supervisor blocks on stdin (the release pipe), then execs the payload
/// in the same process so PID/process-group/start-token stay stable. Pipe
/// closure before release makes `read` fail and the supervisor exit without
/// running the payload.
const LOCAL_SUPERVISOR_SCRIPT: &str = "read -r _ || exit 98; exec sh -c \"$SYMPHONY_COMMAND\"";

#[cfg(unix)]
async fn execute_local(
    request: &CommandExecutionRequest,
    on_launch: impl FnOnce(LaunchIdentity) -> Result<()> + Send,
) -> std::result::Result<CommandExecutionResult, CommandRunFailure> {
    if request.timeout_ms == 0 {
        return Err(CommandRunFailure::SpawnError(
            "command timeout must be greater than zero".to_string(),
        ));
    }

    let env = command_env(
        &request.home_dir,
        &request.workspace_path,
        &request.evidence_dir,
        "attempt",
        &request.command,
    );

    let mut child_cmd = Command::new("sh");
    child_cmd
        .arg("-c")
        .arg(LOCAL_SUPERVISOR_SCRIPT)
        .current_dir(&request.workspace_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(env);

    // The supervisor is its own process group so timeout/restart/cancellation
    // can kill the whole descendant tree.
    child_cmd.process_group(0);

    let mut child = child_cmd.spawn().map_err(|error| {
        CommandRunFailure::SpawnError(format!("failed spawning command supervisor: {error}"))
    })?;
    let child_id = child.id().ok_or_else(|| {
        CommandRunFailure::SpawnError("supervisor exited before identity capture".to_string())
    })?;
    let identity = process_identity::capture_child(child_id, true);
    let stdin = child.stdin.take().ok_or_else(|| {
        CommandRunFailure::SpawnError("supervisor stdin is not piped".to_string())
    })?;

    let started_at = Utc::now();
    // Durable CAS before release: the payload must not run until the recorded
    // identity proves the process is ours to terminate.
    on_launch(LaunchIdentity::Process(identity.clone())).map_err(|error| {
        CommandRunFailure::SpawnError(format!("launch identity was not durably recorded: {error}"))
    })?;

    // Release the payload.
    let mut stdin = stdin;
    let _ = stdin.write_all(b"go\n").await;
    drop(stdin);

    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");
    let stdout_pump = tokio::spawn(async move {
        let mut raw: Vec<u8> = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if raw.len() < 64 * 1024 {
                        raw.extend_from_slice(&buf[..n]);
                    }
                }
            }
        }
        raw
    });
    let stderr_pump = tokio::spawn(async move {
        let mut raw: Vec<u8> = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if raw.len() < 64 * 1024 {
                        raw.extend_from_slice(&buf[..n]);
                    }
                }
            }
        }
        raw
    });

    let wait_result = timeout(
        Duration::from_millis(request.timeout_ms),
        child.wait(),
    )
    .await;
    if let Err(_) = wait_result {
        // Terminate and reap BEFORE draining output: the output pumps only
        // return when the child closes its pipes.
        let outcome = process_identity::terminate_process_group(&identity).await;
        match outcome {
            TerminationOutcome::Terminated => {}
            TerminationOutcome::NoLongerSignalable(reason) => {
                return Err(CommandRunFailure::NotSignalable(format!(
                    "command timed out and its recorded identity became unsignalable: {reason}"
                )));
            }
            TerminationOutcome::StillRunning => {
                return Err(CommandRunFailure::StillRunning(
                    "command timed out and its process group survived termination".to_string(),
                ));
            }
        }
    }
    let stdout_raw = stdout_pump.await.unwrap_or_default();
    let stderr_raw = stderr_pump.await.unwrap_or_default();
    let completed_at = Utc::now();

    let status = match wait_result {
        Ok(Ok(status)) => Some(status),
        Ok(Err(error)) => {
            let _ = process_identity::terminate_process_group(&identity).await;
            return Err(CommandRunFailure::SpawnError(format!(
                "failed waiting for command: {error}"
            )));
        }
        Err(_elapsed) => None,
    };
    let (stdout_tail, stderr_tail) = (
        bounded_redacted_tail(&stdout_raw),
        bounded_redacted_tail(&stderr_raw),
    );
    let output = CommandOutput {
        stdout_tail: stdout_tail.clone(),
        stderr_tail: stderr_tail.clone(),
        output_sha256: output_digest(&stdout_tail, &stderr_tail),
    };
    let Some(status) = status else {
        return Err(CommandRunFailure::TimedOut(output));
    };

    let exit_code = status.code();
    if exit_code == Some(BARRIER_CLOSED_EXIT) {
        return Err(CommandRunFailure::SpawnError(
            "release pipe closed before the payload ran; supervisor exited".to_string(),
        ));
    }
    let passed = status.success();
    let duration_ms = (completed_at - started_at).num_milliseconds().max(0) as u64;

    Ok(CommandExecutionResult {
        exit_code,
        termination_reason: None,
        passed,
        output,
        started_at,
        completed_at,
        duration_ms,
    })
}

#[cfg(not(unix))]
async fn execute_local(
    _request: &CommandExecutionRequest,
    _on_launch: impl FnOnce(LaunchIdentity) -> Result<()> + Send,
) -> std::result::Result<CommandExecutionResult, CommandRunFailure> {
    Err(CommandRunFailure::SpawnError(
        "local command execution requires a unix platform".to_string(),
    ))
}

const DOCKER_LABEL_STAGE: &str = "symphony.stage=verification";
const DOCKER_LABEL_ATTEMPT: &str = "symphony.attempt";
const DOCKER_LABEL_COMMAND: &str = "symphony.command";

/// Remove stopped label-matching verification containers left by a crash
/// before identity persistence/start. Running containers are owned by their
/// recorded attempt and are left for the timeout/restart path.
pub async fn cleanup_stopped_verification_containers() -> Result<u64> {
    let output = Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            DOCKER_LABEL_STAGE,
            "--filter",
            "status=exited",
            "--filter",
            "status=created",
            "--format",
            "{{.ID}}",
        ])
        .output()
        .await
        .map_err(map_docker_io_error)?;
    if !output.status.success() {
        return Err(SymphonyError::DockerContainerFailed(format!(
            "docker ps failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let mut removed = 0u64;
    for container_id in String::from_utf8_lossy(&output.stdout).split_whitespace() {
        let status = Command::new("docker")
            .args(["rm", "-f", container_id])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map_err(map_docker_io_error)?;
        if status.success() {
            removed += 1;
        }
    }
    Ok(removed)
}

async fn execute_docker(
    request: &CommandExecutionRequest,
    on_launch: impl FnOnce(LaunchIdentity) -> Result<()> + Send,
) -> std::result::Result<CommandExecutionResult, CommandRunFailure> {
    let docker = request.docker.as_ref().ok_or_else(|| {
        CommandRunFailure::SpawnError("docker profile requires workspace.docker".to_string())
    })?;
    if request.timeout_ms == 0 {
        return Err(CommandRunFailure::SpawnError(
            "command timeout must be greater than zero".to_string(),
        ));
    }

    let workspace_in_container = "/workspace";
    let evidence_in_container = "/evidence";
    let home_in_container = "/home/symphony";

    let env = command_env(
        Path::new(home_in_container),
        Path::new(workspace_in_container),
        Path::new(evidence_in_container),
        "attempt",
        &request.command,
    );

    // `docker create` (never `run`): the stopped container is the launch
    // barrier until its ID is durably recorded.
    let mut create = Command::new("docker");
    create
        .arg("create")
        .arg("--label")
        .arg(DOCKER_LABEL_STAGE)
        .arg("--label")
        .arg(format!("{DOCKER_LABEL_ATTEMPT}={}", request.attempt_id))
        .arg("--label")
        .arg(format!("{DOCKER_LABEL_COMMAND}={}", request.command_name))
        .arg("-w")
        .arg(workspace_in_container)
        .arg("-v")
        .arg(format!(
            "{}:{workspace_in_container}",
            request.workspace_path.display()
        ))
        .arg("-v")
        .arg(format!(
            "{}:{evidence_in_container}",
            request.evidence_dir.display()
        ));
    for (key, value) in &env {
        create.arg("-e").arg(format!("{key}={value}"));
    }
    let image = docker.image.clone();
    create.arg(&image).arg("sh").arg("-c").arg(&request.command);

    let output = create.output().await.map_err(|error| {
        CommandRunFailure::SpawnError(format!("docker create failed: {error}"))
    })?;
    if !output.status.success() {
        return Err(CommandRunFailure::SpawnError(format!(
            "docker create failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if container_id.is_empty() {
        return Err(CommandRunFailure::SpawnError(
            "docker create succeeded without a container id".to_string(),
        ));
    }

    let started_at = Utc::now();
    // Durable record before the payload starts.
    on_launch(LaunchIdentity::Container {
        container_id: container_id.clone(),
    })
    .map_err(|error| {
        CommandRunFailure::SpawnError(format!(
            "container identity was not durably recorded: {error}"
        ))
    })?;

    let start = Command::new("docker")
        .args(["start", &container_id])
        .output()
        .await
        .map_err(|error| {
            CommandRunFailure::SpawnError(format!("docker start failed: {error}"))
        })?;
    if !start.status.success() {
        let _ = remove_container(&container_id).await;
        return Err(CommandRunFailure::SpawnError(format!(
            "docker start failed: {}",
            String::from_utf8_lossy(&start.stderr).trim()
        )));
    }

    let wait_result = timeout(
        Duration::from_millis(request.timeout_ms),
        Command::new("docker")
            .args(["wait", &container_id])
            .output(),
    )
    .await;
    let completed_at = Utc::now();

    let wait_status = match wait_result {
        Ok(Ok(output)) if output.status.success() => {
            let code = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<i32>()
                .unwrap_or(-1);
            code
        }
        Ok(Ok(output)) => {
            let _ = remove_container(&container_id).await;
            return Err(CommandRunFailure::SpawnError(format!(
                "docker wait failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(Err(error)) => {
            let _ = remove_container(&container_id).await;
            return Err(CommandRunFailure::SpawnError(format!(
                "docker wait failed: {error}"
            )));
        }
        Err(_elapsed) => {
            // Timeout: stop and remove the persisted container, then report.
            let stopped = stop_container(&container_id).await;
            if stopped.is_err() {
                return Err(CommandRunFailure::StillRunning(format!(
                    "command timed out and container {container_id} could not be removed"
                )));
            }
            return Err(CommandRunFailure::TimedOut(CommandOutput {
                stdout_tail: String::new(),
                stderr_tail: String::new(),
                output_sha256: String::new(),
            }));
        }
    };

    let logs = Command::new("docker")
        .args(["logs", "--tail", "200", &container_id])
        .output()
        .await
        .map_err(|error| {
            CommandRunFailure::SpawnError(format!("docker logs failed: {error}"))
        })?;
    let stdout_tail = bounded_redacted_tail(&logs.stdout);
    let stderr_tail = bounded_redacted_tail(&logs.stderr);
    let output_sha256 = output_digest(&stdout_tail, &stderr_tail);
    let _ = remove_container(&container_id).await;

    let passed = wait_status == 0;
    let duration_ms = (completed_at - started_at).num_milliseconds().max(0) as u64;
    Ok(CommandExecutionResult {
        exit_code: Some(wait_status),
        termination_reason: None,
        passed,
        output: CommandOutput {
            stdout_tail,
            stderr_tail,
            output_sha256,
        },
        started_at,
        completed_at,
        duration_ms,
    })
}

async fn stop_container(container_id: &str) -> Result<()> {
    let output = Command::new("docker")
        .args(["stop", "-t", "5", container_id])
        .output()
        .await
        .map_err(map_docker_io_error)?;
    if !output.status.success() {
        return Err(SymphonyError::DockerContainerFailed(format!(
            "docker stop failed for {container_id}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    remove_container(container_id).await
}

async fn remove_container(container_id: &str) -> Result<()> {
    let output = Command::new("docker")
        .args(["rm", "-f", container_id])
        .output()
        .await
        .map_err(map_docker_io_error)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(SymphonyError::DockerContainerFailed(format!(
            "docker rm -f failed for {container_id}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn map_docker_io_error(error: std::io::Error) -> SymphonyError {
    if error.kind() == std::io::ErrorKind::NotFound {
        SymphonyError::DockerNotAvailable
    } else {
        SymphonyError::Io(error)
    }
}

/// Signal-derived termination: 128+n for signal n.
pub fn termination_reason_for_exit(exit_code: Option<i32>) -> Option<String> {
    let code = exit_code?;
    if code >= SIGNAL_BASE {
        Some(format!("terminated by signal {}", code - SIGNAL_BASE))
    } else {
        None
    }
}

/// SHA-256 of the raw command string (recorded per invocation).
pub fn command_sha256(command: &str) -> String {
    hex::encode(Sha256::digest(command.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementation::domain::ExecutionProfile;
    use tempfile::tempdir;

    fn request(command: &str, timeout_ms: u64) -> CommandExecutionRequest {
        let dir = tempdir().unwrap();
        CommandExecutionRequest {
            attempt_id: "attempt-1".to_string(),
            command_name: "test-command".to_string(),
            workspace_path: dir.path().join("workspace"),
            evidence_dir: dir.path().join("evidence"),
            home_dir: dir.path().join("home"),
            command: command.to_string(),
            timeout_ms,
            execution_profile: ExecutionProfile::Local,
            docker: None,
        }
    }

    #[tokio::test]
    async fn local_command_runs_after_launch_identity_is_recorded() {
        let dir = tempdir().unwrap();
        let request = CommandExecutionRequest {
            attempt_id: "attempt-1".to_string(),
            command_name: "test-command".to_string(),
            workspace_path: dir.path().join("workspace"),
            evidence_dir: dir.path().join("evidence"),
            home_dir: dir.path().join("home"),
            command: "printf hello".to_string(),
            timeout_ms: 30_000,
            execution_profile: ExecutionProfile::Local,
            docker: None,
        };
        std::fs::create_dir_all(&request.workspace_path).unwrap();

        let mut recorded: Option<LaunchIdentity> = None;
        let result = execute_command(&request, |identity| {
            assert!(matches!(identity, LaunchIdentity::Process(_)));
            recorded = Some(identity);
            Ok(())
        })
        .await
        .unwrap();

        assert!(result.passed);
        assert_eq!(result.exit_code, Some(0));
        assert!(result.output.stdout_tail.contains("hello"));
        // Identity was captured and handed to the controller before release.
        match recorded {
            Some(LaunchIdentity::Process(identity)) => {
                assert!(identity.pid > 0);
                assert_eq!(identity.process_group_id, identity.pid);
                assert!(identity.start_token.is_some());
            }
            _ => panic!("expected process identity"),
        }
    }

    #[tokio::test]
    async fn launch_identity_failure_never_runs_the_payload() {
        let dir = tempdir().unwrap();
        let request = CommandExecutionRequest {
            attempt_id: "attempt-1".to_string(),
            command_name: "test-command".to_string(),
            workspace_path: dir.path().join("workspace"),
            evidence_dir: dir.path().join("evidence"),
            home_dir: dir.path().join("home"),
            command: "touch payload-ran".to_string(),
            timeout_ms: 30_000,
            execution_profile: ExecutionProfile::Local,
            docker: None,
        };
        std::fs::create_dir_all(&request.workspace_path).unwrap();

        let error = execute_command(&request, |_| {
            Err(SymphonyError::StorageError(
                "CAS failed".to_string(),
            ))
        })
        .await
        .unwrap_err();
        assert!(error.to_string().contains("durably recorded"));
        // The payload must not have executed even though the child existed.
        assert!(!request.workspace_path.join("payload-ran").exists());
    }

    #[tokio::test]
    async fn closed_release_pipe_exits_without_running_payload() {
        // Simulate controller death: drop the stdin pipe without writing.
        let dir = tempdir().unwrap();
        let request = CommandExecutionRequest {
            attempt_id: "attempt-1".to_string(),
            command_name: "test-command".to_string(),
            workspace_path: dir.path().join("workspace"),
            evidence_dir: dir.path().join("evidence"),
            home_dir: dir.path().join("home"),
            command: "touch payload-ran".to_string(),
            timeout_ms: 30_000,
            execution_profile: ExecutionProfile::Local,
            docker: None,
        };
        std::fs::create_dir_all(&request.workspace_path).unwrap();

        // A second execute_command call is not the crash window; instead test
        // the supervisor contract directly: closing stdin without a release
        // line must exit 98 without running the payload.
        use std::process::Stdio as StdStdio;
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg(LOCAL_SUPERVISOR_SCRIPT)
            .current_dir(&request.workspace_path)
            .stdin(StdStdio::piped())
            .stdout(StdStdio::null())
            .stderr(StdStdio::null())
            .env("SYMPHONY_COMMAND", "touch payload-ran")
            .spawn()
            .unwrap();
        drop(child.stdin.take());
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(BARRIER_CLOSED_EXIT));
        assert!(!request.workspace_path.join("payload-ran").exists());
    }

    #[tokio::test]
    async fn timeout_terminates_the_process_group_and_records_interrupted() {
        let dir = tempdir().unwrap();
        let request = CommandExecutionRequest {
            attempt_id: "attempt-1".to_string(),
            command_name: "test-command".to_string(),
            workspace_path: dir.path().join("workspace"),
            evidence_dir: dir.path().join("evidence"),
            home_dir: dir.path().join("home"),
            command: "sleep 60 & wait".to_string(),
            timeout_ms: 1_000,
            execution_profile: ExecutionProfile::Local,
            docker: None,
        };
        std::fs::create_dir_all(&request.workspace_path).unwrap();

        let error = execute_command(&request, |_| Ok(())).await.unwrap_err();
        assert!(
            matches!(error, CommandRunFailure::TimedOut(_)),
            "expected timeout, got: {error}"
        );
    }

    #[tokio::test]
    async fn same_group_descendants_are_terminated_with_the_supervisor() {
        let dir = tempdir().unwrap();
        let marker = dir.path().join("descendant-alive");
        let request = CommandExecutionRequest {
            attempt_id: "attempt-1".to_string(),
            command_name: "test-command".to_string(),
            workspace_path: dir.path().join("workspace"),
            evidence_dir: dir.path().join("evidence"),
            home_dir: dir.path().join("home"),
            command: format!(
                "sh -c 'sleep 5; touch {}' & wait",
                marker.display()
            ),
            timeout_ms: 1_000,
            execution_profile: ExecutionProfile::Local,
            docker: None,
        };
        std::fs::create_dir_all(&request.workspace_path).unwrap();

        let error = execute_command(&request, |_| Ok(())).await.unwrap_err();
        assert!(
            matches!(error, CommandRunFailure::TimedOut(_)),
            "expected timeout, got: {error}"
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            !marker.exists(),
            "same-group descendant must be killed with the supervisor"
        );
    }

    #[tokio::test]
    async fn command_env_omits_forge_and_ssh_credentials() {
        std::env::set_var("GH_TOKEN", "secret-should-not-leak");
        let dir = tempdir().unwrap();
        let request = CommandExecutionRequest {
            attempt_id: "attempt-1".to_string(),
            command_name: "test-command".to_string(),
            workspace_path: dir.path().join("workspace"),
            evidence_dir: dir.path().join("evidence"),
            home_dir: dir.path().join("home"),
            command: "env".to_string(),
            timeout_ms: 30_000,
            execution_profile: ExecutionProfile::Local,
            docker: None,
        };
        std::fs::create_dir_all(&request.workspace_path).unwrap();
        std::fs::create_dir_all(&request.evidence_dir).unwrap();
        std::fs::create_dir_all(&request.home_dir).unwrap();

        let result = execute_command(&request, |_| Ok(())).await.unwrap();
        let env_output = result.output.stdout_tail;
        for forbidden in FORBIDDEN_COMMAND_ENV {
            assert!(
                !env_output.contains(forbidden),
                "command env leaked {forbidden}"
            );
        }
        assert!(env_output.contains("SYMPHONY_EVIDENCE_DIR"));
        std::env::remove_var("GH_TOKEN");
    }

    #[test]
    fn redaction_and_digest_helpers_are_bounded() {
        let secret = format!("https://user:password@github.com/owner/repo");
        let tail = bounded_redacted_tail(secret.as_bytes());
        assert!(!tail.contains("user:password@"));
    }

    #[test]
    fn command_sha256_is_stable() {
        let a = command_sha256("pnpm run test");
        let b = command_sha256("pnpm run test");
        let c = command_sha256("pnpm run lint");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
