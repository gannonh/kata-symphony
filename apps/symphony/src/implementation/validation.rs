//! Deterministic repository validation command execution.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use chrono::Utc;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use crate::error::{Result, SymphonyError};
use crate::implementation::domain::{
    ExecutionProfile, ImplementationValidationCommand, ValidationCommandResult,
};
use crate::repo_url::redact_url_credentials;

const OUTPUT_TAIL_BYTES: usize = 4_096;

/// Result of one ordered validation cycle.
#[derive(Debug, Clone)]
pub struct ValidationCycleOutcome {
    pub passed: bool,
    pub commands: Vec<ValidationCommandResult>,
}

/// Runs configured validation commands sequentially in a workspace.
#[derive(Debug, Clone)]
pub struct ValidationExecutor {
    pub execution_profile: ExecutionProfile,
}

impl Default for ValidationExecutor {
    fn default() -> Self {
        Self {
            execution_profile: ExecutionProfile::Local,
        }
    }
}

impl ValidationExecutor {
    pub fn new(execution_profile: ExecutionProfile) -> Self {
        Self { execution_profile }
    }

    pub fn validate_commands_configured(
        &self,
        commands: &[ImplementationValidationCommand],
    ) -> Result<()> {
        if commands.is_empty() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "implementation.validation must contain at least one command".to_string(),
            ));
        }
        Ok(())
    }

    /// Execute every command in order. Stop the cycle on the first failure.
    pub async fn run_cycle(
        &self,
        workspace: &Path,
        commands: &[ImplementationValidationCommand],
        cycle: u32,
    ) -> Result<ValidationCycleOutcome> {
        self.validate_commands_configured(commands)?;
        let mut results = Vec::with_capacity(commands.len());
        for command in commands {
            let result = self.run_one(workspace, command, cycle).await?;
            let passed = result.passed;
            results.push(result);
            if !passed {
                return Ok(ValidationCycleOutcome {
                    passed: false,
                    commands: results,
                });
            }
        }
        Ok(ValidationCycleOutcome {
            passed: true,
            commands: results,
        })
    }

    async fn run_one(
        &self,
        workspace: &Path,
        command: &ImplementationValidationCommand,
        cycle: u32,
    ) -> Result<ValidationCommandResult> {
        let started_at = Utc::now();
        let wall_start = Instant::now();
        let command_sha256 = hex::encode(Sha256::digest(command.command.as_bytes()));
        let timeout_ms = command.timeout_ms.max(1);
        let argv = split_command(&command.command)?;

        let mut child_cmd = if argv.len() == 1 && looks_like_shell_script(&argv[0]) {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(&command.command);
            cmd
        } else if argv.is_empty() {
            return Err(SymphonyError::TriageError(
                "validation command resolved to an empty argv".to_string(),
            ));
        } else {
            let mut cmd = Command::new(&argv[0]);
            cmd.args(&argv[1..]);
            cmd
        };

        let env = validation_isolated_env(workspace)?;
        child_cmd
            .current_dir(workspace)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env_clear()
            .envs(env);

        let mut child = child_cmd.spawn().map_err(|error| {
            SymphonyError::TriageError(format!(
                "failed to spawn validation command '{}': {error}",
                command.name
            ))
        })?;

        let mut stdout = child.stdout.take().ok_or_else(|| {
            SymphonyError::TriageError(format!(
                "validation command '{}' stdout was not piped",
                command.name
            ))
        })?;
        let mut stderr = child.stderr.take().ok_or_else(|| {
            SymphonyError::TriageError(format!(
                "validation command '{}' stderr was not piped",
                command.name
            ))
        })?;

        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf).await;
            buf
        });
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = stderr.read_to_end(&mut buf).await;
            buf
        });

        let timed = timeout(Duration::from_millis(timeout_ms), child.wait()).await;
        let completed_at = Utc::now();
        let duration_ms = wall_start.elapsed().as_millis() as u64;

        match timed {
            Ok(Ok(status)) => {
                let stdout_bytes = stdout_task.await.unwrap_or_default();
                let stderr_bytes = stderr_task.await.unwrap_or_default();
                let stdout_tail = bounded_redacted_tail(&stdout_bytes);
                let stderr_tail = bounded_redacted_tail(&stderr_bytes);
                let output_sha256 = output_digest(&stdout_tail, &stderr_tail);
                let exit_code = status.code();
                let passed = status.success();
                Ok(ValidationCommandResult {
                    name: command.name.clone(),
                    command_sha256,
                    cycle,
                    started_at,
                    completed_at,
                    duration_ms,
                    exit_code,
                    passed,
                    termination_reason: if passed {
                        None
                    } else {
                        Some(format!(
                            "exit {}",
                            exit_code
                                .map(|code| code.to_string())
                                .unwrap_or_else(|| "signal".to_string())
                        ))
                    },
                    stdout_tail,
                    stderr_tail,
                    output_sha256,
                    execution_profile: self.execution_profile,
                })
            }
            Ok(Err(error)) => {
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                Err(SymphonyError::TriageError(format!(
                    "validation command '{}' failed to wait: {error}",
                    command.name
                )))
            }
            Err(_elapsed) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let stdout_bytes = stdout_task.await.unwrap_or_default();
                let stderr_bytes = stderr_task.await.unwrap_or_default();
                let stdout_tail = bounded_redacted_tail(&stdout_bytes);
                let mut stderr_tail = bounded_redacted_tail(&stderr_bytes);
                let timeout_note = format!(
                    "validation command '{}' timed out after {timeout_ms} ms",
                    command.name
                );
                if stderr_tail.is_empty() {
                    stderr_tail = timeout_note;
                } else {
                    stderr_tail = format!("{stderr_tail}\n{timeout_note}");
                    stderr_tail = tail_str(&stderr_tail, OUTPUT_TAIL_BYTES);
                }
                let output_sha256 = output_digest(&stdout_tail, &stderr_tail);
                Ok(ValidationCommandResult {
                    name: command.name.clone(),
                    command_sha256,
                    cycle,
                    started_at,
                    completed_at,
                    duration_ms,
                    exit_code: None,
                    passed: false,
                    termination_reason: Some("timeout".to_string()),
                    stdout_tail,
                    stderr_tail,
                    output_sha256,
                    execution_profile: self.execution_profile,
                })
            }
        }
    }
}

/// Credential-isolated env for repo validation commands (no forge/SSH/model keys).
pub fn validation_isolated_env(workspace: &Path) -> Result<HashMap<String, String>> {
    let parent: HashMap<String, String> = std::env::vars().collect();
    let mut env = HashMap::new();

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
    }

    let home = disposable_validation_home(workspace)?;
    env.insert("HOME".to_string(), home.display().to_string());
    Ok(env)
}

fn disposable_validation_home(workspace: &Path) -> Result<PathBuf> {
    let home = workspace.join(".symphony-validation-home");
    fs::create_dir_all(&home).map_err(|error| {
        SymphonyError::TriageError(format!(
            "failed creating validation home {}: {error}",
            home.display()
        ))
    })?;
    Ok(home)
}

fn split_command(command: &str) -> Result<Vec<String>> {
    // Prefer shell-words when the string is a simple argv; fall back to `sh -c`
    // for operators that shell_words cannot express (`&&`, pipes, redirects).
    if command.contains(['|', ';', '>', '<'])
        || command.contains("&&")
        || command.contains("||")
        || command.contains("$(")
    {
        return Ok(vec![command.to_string()]);
    }
    shell_words::split(command).map_err(|error| {
        SymphonyError::InvalidWorkflowConfig(format!("invalid validation command quoting: {error}"))
    })
}

fn looks_like_shell_script(first: &str) -> bool {
    first.contains(['|', ';', '>', '<'])
        || first.contains("&&")
        || first.contains("||")
        || first.contains("$(")
}

fn bounded_redacted_tail(bytes: &[u8]) -> String {
    let lossy = String::from_utf8_lossy(bytes);
    let redacted = redact_url_credentials(&lossy);
    tail_str(&redacted, OUTPUT_TAIL_BYTES)
}

fn tail_str(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let start = value.len() - max_bytes;
    // Avoid splitting a UTF-8 codepoint.
    let mut idx = start;
    while idx < value.len() && !value.is_char_boundary(idx) {
        idx += 1;
    }
    value[idx..].to_string()
}

fn output_digest(stdout: &str, stderr: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(stdout.as_bytes());
    hasher.update(b"\0");
    hasher.update(stderr.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn cmd(name: &str, command: &str, timeout_ms: u64) -> ImplementationValidationCommand {
        ImplementationValidationCommand {
            name: name.to_string(),
            command: command.to_string(),
            timeout_ms,
        }
    }

    #[tokio::test]
    async fn validation_cycle_passes_all_commands() {
        let dir = tempdir().unwrap();
        let executor = ValidationExecutor::new(ExecutionProfile::Local);
        let outcome = executor
            .run_cycle(
                dir.path(),
                &[
                    cmd("true-1", "true", 5_000),
                    cmd("echo-ok", "echo ok", 5_000),
                ],
                1,
            )
            .await
            .unwrap();
        assert!(outcome.passed);
        assert_eq!(outcome.commands.len(), 2);
        assert!(outcome.commands.iter().all(|c| c.passed));
        assert_eq!(outcome.commands[0].exit_code, Some(0));
        assert!(!outcome.commands[0].command_sha256.is_empty());
        assert!(!outcome.commands[0].output_sha256.is_empty());
    }

    #[tokio::test]
    async fn validation_cycle_stops_on_first_failure() {
        let dir = tempdir().unwrap();
        let executor = ValidationExecutor::new(ExecutionProfile::Local);
        let outcome = executor
            .run_cycle(
                dir.path(),
                &[
                    cmd("pass", "true", 5_000),
                    cmd("fail", "false", 5_000),
                    cmd("never", "true", 5_000),
                ],
                2,
            )
            .await
            .unwrap();
        assert!(!outcome.passed);
        assert_eq!(outcome.commands.len(), 2);
        assert!(outcome.commands[0].passed);
        assert!(!outcome.commands[1].passed);
        assert_eq!(outcome.commands[1].exit_code, Some(1));
        assert_eq!(outcome.commands[1].cycle, 2);
    }

    #[tokio::test]
    async fn validation_runs_command_substitution_through_shell() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("README.md"), "# Symphony\n").unwrap();
        let executor = ValidationExecutor::new(ExecutionProfile::Local);
        let outcome = executor
            .run_cycle(
                dir.path(),
                &[cmd(
                    "readme-heading",
                    r##"test "$(head -n 1 README.md)" = "# Symphony""##,
                    5_000,
                )],
                1,
            )
            .await
            .unwrap();

        assert!(outcome.passed);
        assert_eq!(outcome.commands[0].exit_code, Some(0));
    }

    #[tokio::test]
    async fn validation_command_timeout_is_recorded() {
        let dir = tempdir().unwrap();
        let executor = ValidationExecutor::new(ExecutionProfile::Local);
        let outcome = executor
            .run_cycle(dir.path(), &[cmd("sleep", "sleep 5", 200)], 1)
            .await
            .unwrap();
        assert!(!outcome.passed);
        assert_eq!(outcome.commands.len(), 1);
        assert!(!outcome.commands[0].passed);
        assert_eq!(
            outcome.commands[0].termination_reason.as_deref(),
            Some("timeout")
        );
        assert!(outcome.commands[0].exit_code.is_none());
    }

    #[tokio::test]
    async fn validation_timeout_preserves_stdout_tail() {
        let dir = tempdir().unwrap();
        let executor = ValidationExecutor::new(ExecutionProfile::Local);
        let outcome = executor
            .run_cycle(
                dir.path(),
                &[cmd("slow", "echo before-timeout; sleep 5", 300)],
                1,
            )
            .await
            .unwrap();
        assert!(!outcome.passed);
        assert_eq!(
            outcome.commands[0].termination_reason.as_deref(),
            Some("timeout")
        );
        assert!(
            outcome.commands[0].stdout_tail.contains("before-timeout"),
            "expected preserved stdout, got {:?}",
            outcome.commands[0].stdout_tail
        );
    }

    #[tokio::test]
    async fn validation_does_not_inherit_gh_token() {
        let dir = tempdir().unwrap();
        let secret = "super-secret-gh-token-xyz";
        std::env::set_var("GH_TOKEN", secret);
        let executor = ValidationExecutor::new(ExecutionProfile::Local);
        let outcome = executor
            .run_cycle(
                dir.path(),
                &[cmd("check-token", "printenv GH_TOKEN; exit 0", 5_000)],
                1,
            )
            .await
            .unwrap();
        std::env::remove_var("GH_TOKEN");
        assert!(outcome.passed);
        assert!(
            !outcome.commands[0].stdout_tail.contains(secret),
            "GH_TOKEN leaked into validation env: {:?}",
            outcome.commands[0].stdout_tail
        );
        // Empty printenv produces no stdout.
        assert!(!outcome.commands[0].stdout_tail.contains("GH_TOKEN"));
    }

    #[test]
    fn validation_env_omits_forge_model_and_ssh() {
        std::env::set_var("GH_TOKEN", "gh");
        std::env::set_var("ANTHROPIC_API_KEY", "sk");
        std::env::set_var("OPENAI_API_KEY", "sk-oai");
        std::env::set_var("SSH_AUTH_SOCK", "/tmp/ssh");
        let dir = tempdir().unwrap();
        let env = validation_isolated_env(dir.path()).unwrap();
        std::env::remove_var("GH_TOKEN");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("SSH_AUTH_SOCK");
        assert!(!env.contains_key("GH_TOKEN"));
        assert!(!env.contains_key("ANTHROPIC_API_KEY"));
        assert!(!env.contains_key("OPENAI_API_KEY"));
        assert!(!env.contains_key("SSH_AUTH_SOCK"));
        assert!(env.contains_key("HOME"));
        assert!(env
            .get("HOME")
            .unwrap()
            .contains("symphony-validation-home"));
    }
}
