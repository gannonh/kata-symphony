use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::{timeout, Instant};

use crate::error::Result;
use crate::triage::artifact::{self, ArtifactValidationError};
use crate::triage::domain::{StageUsage, TriageArtifact};
use crate::triage::integrity::{self, RepoBaseline};

const FORCE_KILL_WAIT: Duration = Duration::from_secs(5);
const OUTPUT_ENV: &str = "SYMPHONY_STAGE_OUTPUT";
const MODEL_ENV: &str = "SYMPHONY_TRIAGE_MODEL";

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

#[derive(Debug, Clone)]
pub struct TriageRunnerRequest {
    pub attempt_id: String,
    pub workspace_root: PathBuf,
    pub repo_path: PathBuf,
    pub command: Vec<String>,
    pub turn_timeout_ms: u64,
    /// Effective Pi model. Codex callers pass `None`.
    pub model: Option<String>,
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
#[allow(clippy::large_enum_variant)]
pub enum TriageRunnerOutcome {
    Success(TriageRunnerSuccess),
    Failure(TriageRunnerFailure),
}

pub struct TriageRunner;

impl TriageRunner {
    pub async fn run(request: TriageRunnerRequest) -> Result<TriageRunnerOutcome> {
        if request.command.is_empty() {
            return Ok(failure(
                TriageRunnerFailureKind::Setup,
                "triage command cannot be empty",
            ));
        }
        if request.turn_timeout_ms == 0 {
            return Ok(failure(
                TriageRunnerFailureKind::Setup,
                "triage turn_timeout_ms must be greater than zero",
            ));
        }
        if !request.repo_path.is_dir() {
            return Ok(failure(
                TriageRunnerFailureKind::Setup,
                format!(
                    "triage repo path does not exist: {}",
                    request.repo_path.display()
                ),
            ));
        }

        let attempt_root = request
            .workspace_root
            .join(format!("triage-{}", request.attempt_id));
        let workspace_path = attempt_root.join("workspace");
        let output_dir = attempt_root.join("stage-output");
        let output_path = output_dir.join("result.json");
        let home_dir = attempt_root.join("home");

        if let Err(err) = prepare_dirs(&workspace_path, &output_dir, &home_dir) {
            return Ok(failure(TriageRunnerFailureKind::Setup, err.to_string()));
        }

        if let Err(err) = clone_local_workspace(&request.repo_path, &workspace_path) {
            let _ = fs::remove_dir_all(&attempt_root);
            return Ok(failure(TriageRunnerFailureKind::Setup, err));
        }

        if let Err(err) = disable_push_urls(&workspace_path) {
            let _ = fs::remove_dir_all(&attempt_root);
            return Ok(failure(TriageRunnerFailureKind::Setup, err));
        }

        let baseline = match integrity::capture_baseline(&workspace_path) {
            Ok(baseline) => baseline,
            Err(err) => {
                let _ = fs::remove_dir_all(&attempt_root);
                return Ok(failure(TriageRunnerFailureKind::Setup, err.to_string()));
            }
        };

        let mut command_args = request.command.clone();
        if let Some(model) = request.model.as_deref() {
            if is_pi_like_command(&command_args) && !command_has_model_flag(&command_args) {
                command_args.push("--model".to_string());
                command_args.push(model.to_string());
            }
        }

        let env = build_isolated_env(&home_dir, &output_path, request.model.as_deref());
        let program = command_args[0].clone();
        let args = &command_args[1..];

        let mut child_cmd = Command::new(&program);
        child_cmd
            .args(args)
            .current_dir(&workspace_path)
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
                let _ = fs::remove_dir_all(&attempt_root);
                return Ok(failure(
                    TriageRunnerFailureKind::Spawn,
                    format!("failed to spawn triage command '{program}': {err}"),
                ));
            }
        };

        let child_id = child.id();
        let turn_timeout = Duration::from_millis(request.turn_timeout_ms);
        let wait_result = timeout(turn_timeout, child.wait()).await;

        let status = match wait_result {
            Ok(Ok(status)) => status,
            Ok(Err(err)) => {
                let _ = fs::remove_dir_all(&attempt_root);
                return Ok(failure(
                    TriageRunnerFailureKind::Spawn,
                    format!("failed waiting for triage command: {err}"),
                ));
            }
            Err(_elapsed) => {
                terminate_process_group(child_id).await;
                let _ = timeout(Duration::from_millis(100), child.wait()).await;
                let _ = fs::remove_dir_all(&attempt_root);
                return Ok(failure(
                    TriageRunnerFailureKind::Timeout,
                    format!(
                        "triage turn exceeded turn_timeout_ms={}",
                        request.turn_timeout_ms
                    ),
                ));
            }
        };

        if !status.success() {
            let code = status.code().unwrap_or(-1);
            let _ = fs::remove_dir_all(&attempt_root);
            return Ok(failure(
                TriageRunnerFailureKind::NonZeroExit,
                format!("triage command exited with status {code}"),
            ));
        }

        let bytes = match fs::read(&output_path) {
            Ok(bytes) => bytes,
            Err(err) => {
                let _ = fs::remove_dir_all(&attempt_root);
                return Ok(failure(
                    TriageRunnerFailureKind::MissingOutput,
                    format!("missing triage output at {}: {err}", output_path.display()),
                ));
            }
        };

        let artifact = match artifact::parse_and_validate(&bytes) {
            Ok(artifact) => artifact,
            Err(err) => {
                let _ = fs::remove_dir_all(&attempt_root);
                return Ok(failure(
                    TriageRunnerFailureKind::InvalidArtifact,
                    format_artifact_error(err),
                ));
            }
        };

        if let Err(err) = integrity::check_repository_integrity(&workspace_path, &baseline) {
            let _ = fs::remove_dir_all(&attempt_root);
            return Ok(failure(TriageRunnerFailureKind::Integrity, err.to_string()));
        }

        Ok(TriageRunnerOutcome::Success(TriageRunnerSuccess {
            artifact,
            usage: StageUsage::default(),
            workspace_path,
            output_path,
            baseline,
        }))
    }
}

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

        let workspace_root = temp.path().join("workspaces");
        let outcome = TriageRunner::run(TriageRunnerRequest {
            attempt_id: "attempt-1".to_string(),
            workspace_root,
            repo_path: repo,
            command: vec![script.display().to_string()],
            turn_timeout_ms: 5_000,
            model: Some("anthropic/claude-sonnet-4-6".to_string()),
        })
        .await
        .unwrap();

        let success = match outcome {
            TriageRunnerOutcome::Success(success) => success,
            TriageRunnerOutcome::Failure(failure) => {
                panic!("expected success, got {:?}", failure);
            }
        };
        assert_eq!(success.artifact.route.as_str(), "implement");
        assert_eq!(success.usage.total_tokens, 0);

        let env_check = fs::read_to_string(
            success
                .workspace_path
                .parent()
                .unwrap()
                .join("home/env-check.txt"),
        )
        .unwrap();
        assert!(env_check.contains("GH_TOKEN=\n"));
        assert!(env_check.contains("GITHUB_TOKEN=\n"));
        assert!(env_check.contains("SSH_AUTH_SOCK=\n"));
        assert!(env_check.contains("SYMPHONY_BIN=\n"));
        assert!(env_check.contains("MODEL=anthropic/claude-sonnet-4-6"));

        let push = StdCommand::new("git")
            .args(["remote", "get-url", "--push", "origin"])
            .current_dir(&success.workspace_path)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&push.stdout).trim(), "DISABLED");
    }

    #[tokio::test]
    async fn timeout_kills_long_running_command() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        init_repo(&repo);
        let script = temp.path().join("sleep.sh");
        write_script(&script, "#!/bin/sh\nsleep 30\n");

        let outcome = TriageRunner::run(TriageRunnerRequest {
            attempt_id: "timeout".to_string(),
            workspace_root: temp.path().join("workspaces"),
            repo_path: repo,
            command: vec![script.display().to_string()],
            turn_timeout_ms: 200,
            model: None,
        })
        .await
        .unwrap();

        match outcome {
            TriageRunnerOutcome::Failure(failure) => {
                assert_eq!(failure.kind, TriageRunnerFailureKind::Timeout);
            }
            TriageRunnerOutcome::Success(_) => panic!("expected timeout"),
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

        let outcome = TriageRunner::run(TriageRunnerRequest {
            attempt_id: "dirty".to_string(),
            workspace_root: temp.path().join("workspaces"),
            repo_path: repo,
            command: vec![script.display().to_string()],
            turn_timeout_ms: 5_000,
            model: None,
        })
        .await
        .unwrap();

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

        let outcome = TriageRunner::run(TriageRunnerRequest {
            attempt_id: "ignored".to_string(),
            workspace_root: temp.path().join("workspaces"),
            repo_path: repo,
            command: vec![script.display().to_string()],
            turn_timeout_ms: 5_000,
            model: None,
        })
        .await
        .unwrap();

        assert!(matches!(outcome, TriageRunnerOutcome::Success(_)));
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
