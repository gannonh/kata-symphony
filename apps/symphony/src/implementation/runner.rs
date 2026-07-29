//! Local and Docker implementation worker invocation.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use crate::domain::CodexConfig;
use crate::error::{Result, SymphonyError};
use crate::implementation::bundle::{clone_from_bundle, create_base_bundle, verify_bundle};
use crate::implementation::domain::ExecutionProfile;
use crate::triage::domain::StageUsage;
use crate::triage::runner::{
    build_isolated_env, disable_push_urls, prepare_dirs, run_codex_turn, run_pi_turn,
    AttemptLayout, TriageHarness, TriageIssueIdentity, TriageRunnerOutcome, TriageRunnerRequest,
    TriageSpawnSink,
};

const OUTPUT_ENV: &str = "SYMPHONY_STAGE_OUTPUT";
const FORBIDDEN_DOCKER_ENV: &[&str] = &[
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "LINEAR_API_KEY",
    "LINEAR_API_TOKEN",
    "SYMPHONY_HELPER_TOKEN",
    "SSH_AUTH_SOCK",
    "SSH_AGENT_PID",
];

/// Request for one implementation or repair turn.
#[derive(Clone)]
pub struct ImplementationTurnRequest {
    pub attempt_id: String,
    pub workspace_root: PathBuf,
    /// Trusted source repository used to create the base bundle.
    pub repo_path: PathBuf,
    pub base_commit: String,
    pub command: Vec<String>,
    pub prompt: String,
    pub timeout_ms: u64,
    pub model: Option<String>,
    pub harness: TriageHarness,
    pub issue: TriageIssueIdentity,
    pub codex: Option<CodexConfig>,
    pub approved_spec_markdown: String,
    pub approved_spec_path: String,
    pub stage_inputs: serde_json::Value,
    pub spawned: Option<TriageSpawnSink>,
    pub execution_profile: ExecutionProfile,
    /// When set, reuse an existing attempt workspace (repair turns).
    pub existing_workspace: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ImplementationTurnResult {
    pub workspace_path: PathBuf,
    pub output_path: PathBuf,
    pub output_bytes: Vec<u8>,
    pub usage: StageUsage,
    pub attempt_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct PostconditionAssessment {
    pub head_commit: String,
    pub tree_sha: String,
    pub changed_paths: Vec<String>,
}

/// Injectable harness so coordinator tests can avoid spawning Pi/Codex.
#[async_trait]
pub trait ImplementationHarness: Send + Sync {
    async fn run_turn(
        &self,
        request: &ImplementationTurnRequest,
        layout: &AttemptLayout,
    ) -> Result<StageUsage>;
}

/// Real Pi/Codex harness using A1 isolation helpers.
pub struct LiveImplementationHarness;

#[async_trait]
impl ImplementationHarness for LiveImplementationHarness {
    async fn run_turn(
        &self,
        request: &ImplementationTurnRequest,
        layout: &AttemptLayout,
    ) -> Result<StageUsage> {
        let triage_request = TriageRunnerRequest {
            attempt_id: request.attempt_id.clone(),
            workspace_root: request.workspace_root.clone(),
            repo_path: request.repo_path.clone(),
            command: request.command.clone(),
            prompt: request.prompt.clone(),
            turn_timeout_ms: request.timeout_ms,
            model: request.model.clone(),
            harness: request.harness,
            issue: request.issue.clone(),
            codex: request.codex.clone(),
            progress: None,
            spawned: request.spawned.clone(),
        };
        let usage = match request.harness {
            TriageHarness::Pi => run_pi_turn(&triage_request, layout).await,
            TriageHarness::Codex => run_codex_turn(&triage_request, layout).await,
        }
        .map_err(|outcome| {
            SymphonyError::TriageError(format!(
                "implementation turn failed: {}",
                runner_outcome_message(&outcome)
            ))
        })?;
        Ok(usage)
    }
}

/// Local (and Docker-profile) implementation runner.
#[derive(Default)]
pub struct ImplementationRunner;

impl ImplementationRunner {
    pub async fn run_local_turn<H: ImplementationHarness>(
        &self,
        request: &ImplementationTurnRequest,
        harness: &H,
    ) -> Result<ImplementationTurnResult> {
        if request.timeout_ms == 0 {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "implementation invocation_timeout_ms must be greater than zero".to_string(),
            ));
        }
        if request.approved_spec_path.trim().is_empty() {
            return Err(SymphonyError::InvalidWorkflowConfig(
                "approved spec path is required".to_string(),
            ));
        }

        let layout = if let Some(existing) = request.existing_workspace.as_ref() {
            repair_layout(request, existing)?
        } else {
            prepare_local_attempt(request)?
        };

        // Materialize approved spec before the worker starts (do not commit).
        let spec_path = layout.workspace_path.join(&request.approved_spec_path);
        if let Some(parent) = spec_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                SymphonyError::TriageError(format!(
                    "failed creating approved spec parent {}: {error}",
                    parent.display()
                ))
            })?;
        }
        fs::write(&spec_path, request.approved_spec_markdown.as_bytes()).map_err(|error| {
            SymphonyError::TriageError(format!(
                "failed writing approved spec {}: {error}",
                spec_path.display()
            ))
        })?;

        write_stage_inputs(&layout.stage_input_path, &request.stage_inputs)?;

        let usage = harness.run_turn(request, &layout).await.map_err(|error| {
            let _ = fs::remove_dir_all(&layout.attempt_root);
            error
        })?;

        let output_bytes = fs::read(&layout.output_path).map_err(|error| {
            let _ = fs::remove_dir_all(&layout.attempt_root);
            SymphonyError::TriageError(format!(
                "missing implementation output at {}: {error}",
                layout.output_path.display()
            ))
        })?;

        Ok(ImplementationTurnResult {
            workspace_path: layout.workspace_path,
            output_path: layout.output_path,
            output_bytes,
            usage,
            attempt_root: layout.attempt_root,
        })
    }
}

fn prepare_local_attempt(request: &ImplementationTurnRequest) -> Result<AttemptLayout> {
    let attempt_root = request
        .workspace_root
        .join(format!("attempt-{}", request.attempt_id));
    let layout = AttemptLayout {
        attempt_root: attempt_root.clone(),
        workspace_path: attempt_root.join("workspace"),
        output_path: attempt_root.join("stage-output").join("result.json"),
        stage_input_path: attempt_root.join("stage-input"),
        home_dir: attempt_root.join("home"),
    };

    prepare_dirs(
        &layout.workspace_path,
        layout.output_path.parent().expect("output dir"),
        &layout.home_dir,
    )
    .map_err(|error| {
        SymphonyError::TriageError(format!("failed preparing attempt dirs: {error}"))
    })?;
    fs::create_dir_all(&layout.stage_input_path).map_err(|error| {
        SymphonyError::TriageError(format!("failed creating stage input dir: {error}"))
    })?;

    // Bundle-backed clone — never the shared worktree.
    let bundle_path = attempt_root.join("base.bundle");
    if let Err(error) = create_base_bundle(&request.repo_path, &request.base_commit, &bundle_path) {
        let _ = fs::remove_dir_all(&attempt_root);
        return Err(error);
    }
    // clone_from_bundle requires dest absent; prepare_dirs created an empty workspace.
    let _ = fs::remove_dir_all(&layout.workspace_path);
    if let Err(error) = clone_from_bundle(&bundle_path, &layout.workspace_path) {
        let _ = fs::remove_dir_all(&attempt_root);
        return Err(error);
    }
    // Ensure HEAD is the captured base (clone may land on a branch tip).
    if let Err(error) = git_checkout(&layout.workspace_path, &request.base_commit) {
        let _ = fs::remove_dir_all(&attempt_root);
        return Err(SymphonyError::TriageError(error));
    }
    configure_local_identity(&layout.workspace_path)?;
    if let Err(error) = disable_push_urls(&layout.workspace_path) {
        let _ = fs::remove_dir_all(&attempt_root);
        return Err(SymphonyError::TriageError(error));
    }
    Ok(layout)
}

fn repair_layout(
    request: &ImplementationTurnRequest,
    existing_workspace: &Path,
) -> Result<AttemptLayout> {
    let attempt_root = existing_workspace
        .parent()
        .ok_or_else(|| {
            SymphonyError::TriageError("repair workspace has no parent attempt root".to_string())
        })?
        .to_path_buf();
    let layout = AttemptLayout {
        attempt_root: attempt_root.clone(),
        workspace_path: existing_workspace.to_path_buf(),
        output_path: attempt_root.join("stage-output").join("result.json"),
        stage_input_path: attempt_root.join("stage-input"),
        home_dir: attempt_root.join("home"),
    };
    if let Some(parent) = layout.output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            SymphonyError::TriageError(format!("failed creating repair output dir: {error}"))
        })?;
    }
    fs::create_dir_all(&layout.stage_input_path).map_err(|error| {
        SymphonyError::TriageError(format!("failed creating repair stage input: {error}"))
    })?;
    fs::create_dir_all(&layout.home_dir).map_err(|error| {
        SymphonyError::TriageError(format!("failed creating repair home: {error}"))
    })?;
    // Clear prior output so a missing write cannot reuse a stale manifest.
    let _ = fs::remove_file(&layout.output_path);
    let _ = request;
    Ok(layout)
}

fn write_stage_inputs(path: &Path, inputs: &serde_json::Value) -> Result<()> {
    fs::create_dir_all(path).map_err(|error| {
        SymphonyError::TriageError(format!("failed creating stage inputs: {error}"))
    })?;
    let file = path.join("inputs.json");
    let bytes = serde_json::to_vec_pretty(inputs).map_err(|error| {
        SymphonyError::TriageError(format!("failed encoding stage inputs: {error}"))
    })?;
    fs::write(&file, bytes).map_err(|error| {
        SymphonyError::TriageError(format!(
            "failed writing stage inputs {}: {error}",
            file.display()
        ))
    })?;
    Ok(())
}

fn configure_local_identity(workspace: &Path) -> Result<()> {
    for (key, value) in [
        ("user.email", "symphony-implementation@localhost"),
        ("user.name", "Symphony Implementation"),
    ] {
        let output = Command::new("git")
            .args(["config", key, value])
            .current_dir(workspace)
            .output()
            .map_err(|error| {
                SymphonyError::TriageError(format!("failed setting git {key}: {error}"))
            })?;
        if !output.status.success() {
            return Err(SymphonyError::TriageError(format!(
                "failed setting git {key}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
    }
    Ok(())
}

fn git_checkout(workspace: &Path, commit: &str) -> std::result::Result<(), String> {
    let output = Command::new("git")
        .args(["checkout", "--detach", commit])
        .current_dir(workspace)
        .output()
        .map_err(|error| format!("git checkout failed: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git checkout failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn runner_outcome_message(outcome: &TriageRunnerOutcome) -> String {
    match outcome {
        TriageRunnerOutcome::Failure(failure) => failure.message.clone(),
        TriageRunnerOutcome::Success(_) => "unexpected runner success".to_string(),
    }
}

/// Assess repository postconditions after a completed implementation turn.
pub fn assess_postconditions(
    workspace: &Path,
    base_commit: &str,
    approved_spec_path: &str,
    canonical_spec_bytes: &[u8],
    expected_head: &str,
) -> Result<PostconditionAssessment> {
    let head = git_stdout(workspace, &["rev-parse", "HEAD"])?;
    if head != expected_head {
        return Err(SymphonyError::TriageError(format!(
            "HEAD {head} does not equal manifest head_commit {expected_head}"
        )));
    }
    let merge_base = git_stdout(workspace, &["merge-base", base_commit, "HEAD"])?;
    if merge_base != base_commit {
        return Err(SymphonyError::TriageError(format!(
            "HEAD does not descend from base commit {base_commit} (merge-base={merge_base})"
        )));
    }
    let status = git_stdout(
        workspace,
        &["status", "--porcelain", "--untracked-files=normal"],
    )?;
    // Allow ignored files only — porcelain output must be empty.
    if !status.trim().is_empty() {
        return Err(SymphonyError::TriageError(format!(
            "working tree is not clean:\n{status}"
        )));
    }
    let spec_path = workspace.join(approved_spec_path);
    let committed = fs::read(&spec_path).map_err(|error| {
        SymphonyError::TriageError(format!(
            "approved spec missing at {}: {error}",
            spec_path.display()
        ))
    })?;
    if committed != canonical_spec_bytes {
        return Err(SymphonyError::TriageError(format!(
            "approved spec at {approved_spec_path} is not byte-identical to the canonical render"
        )));
    }
    // Confirm the path is a regular file (not a symlink).
    let meta = fs::symlink_metadata(&spec_path).map_err(|error| {
        SymphonyError::TriageError(format!("failed stating approved spec: {error}"))
    })?;
    if meta.file_type().is_symlink() {
        return Err(SymphonyError::TriageError(
            "approved spec path must not be a symlink".to_string(),
        ));
    }

    let changed = git_stdout(
        workspace,
        &["diff", "--name-only", &format!("{base_commit}..HEAD")],
    )?;
    let changed_paths: Vec<String> = changed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    let non_spec = changed_paths.iter().any(|path| path != approved_spec_path);
    if !non_spec {
        return Err(SymphonyError::TriageError(
            "implementation must change at least one path other than the approved spec".to_string(),
        ));
    }
    let tree_sha = git_stdout(workspace, &["rev-parse", "HEAD^{tree}"])?;
    Ok(PostconditionAssessment {
        head_commit: head,
        tree_sha,
        changed_paths,
    })
}

fn git_stdout(workspace: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .map_err(|error| {
            SymphonyError::TriageError(format!("git {} failed: {error}", args.join(" ")))
        })?;
    if !output.status.success() {
        return Err(SymphonyError::TriageError(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Build the environment list that would be passed to `docker run` for A3.
///
/// Model auth only — forge, Linear, helper, and SSH credentials are omitted.
pub fn docker_credential_isolated_env(
    home_in_container: &str,
    output_in_container: &str,
    model: Option<&str>,
    parent_env: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Some(path) = parent_env.get("PATH") {
        env.push(("PATH".to_string(), path.clone()));
    }
    for key in ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "CLAUDE_API_KEY"] {
        if let Some(value) = parent_env.get(key) {
            env.push((key.to_string(), value.clone()));
        }
    }
    env.push(("HOME".to_string(), home_in_container.to_string()));
    env.push((OUTPUT_ENV.to_string(), output_in_container.to_string()));
    if let Some(model) = model {
        env.push(("SYMPHONY_TRIAGE_MODEL".to_string(), model.to_string()));
    }
    env.retain(|(key, _)| !FORBIDDEN_DOCKER_ENV.contains(&key.as_str()));
    env
}

/// Assert helper used by tests and doctor: ensure forbidden keys are absent.
pub fn assert_no_forge_credentials(env: &[(String, String)]) -> Result<()> {
    for (key, _) in env {
        if FORBIDDEN_DOCKER_ENV
            .iter()
            .any(|forbidden| forbidden == key)
        {
            return Err(SymphonyError::TriageError(format!(
                "forbidden credential {key} present in docker env"
            )));
        }
    }
    Ok(())
}

/// SHA-256 of committed blob at `path` (for bundle metadata).
pub fn blob_sha_of_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).map_err(|error| {
        SymphonyError::TriageError(format!("failed reading {}: {error}", path.display()))
    })?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

/// Resolve `base_branch` SHA from a trusted local repository.
pub fn resolve_base_commit(repo: &Path, base_branch: &str) -> Result<String> {
    let candidates = [
        format!("refs/heads/{base_branch}"),
        format!("refs/remotes/origin/{base_branch}"),
        base_branch.to_string(),
    ];
    for candidate in candidates {
        if let Ok(sha) = git_stdout(repo, &["rev-parse", "--verify", &candidate]) {
            if sha.len() >= 40 {
                return Ok(sha);
            }
        }
    }
    Err(SymphonyError::TriageError(format!(
        "could not resolve workspace.base_branch '{base_branch}' in {}",
        repo.display()
    )))
}

/// Prove the isolated env builder omits forge credentials (shared with triage).
pub fn isolated_env_omits_forge_credentials(
    home_dir: &Path,
    output_path: &Path,
) -> HashMap<String, String> {
    build_isolated_env(home_dir, output_path, None)
}

pub use crate::implementation::bundle::verify_bundle as verify_git_bundle;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementation::bundle::create_base_bundle;
    use tempfile::tempdir;

    fn init_repo() -> (tempfile::TempDir, String) {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        for args in [
            ["init"].as_slice(),
            ["config", "user.email", "t@example.com"].as_slice(),
            ["config", "user.name", "T"].as_slice(),
        ] {
            assert!(Command::new("git")
                .args(args)
                .current_dir(repo)
                .status()
                .unwrap()
                .success());
        }
        fs::write(repo.join("README.md"), "base\n").unwrap();
        assert!(Command::new("git")
            .args(["add", "README.md"])
            .current_dir(repo)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(repo)
            .status()
            .unwrap()
            .success());
        let head = git_stdout(repo, &["rev-parse", "HEAD"]).unwrap();
        (dir, head)
    }

    struct CommitHarness {
        spec_path: String,
        spec_bytes: Vec<u8>,
    }

    #[async_trait]
    impl ImplementationHarness for CommitHarness {
        async fn run_turn(
            &self,
            _request: &ImplementationTurnRequest,
            layout: &AttemptLayout,
        ) -> Result<StageUsage> {
            let code = layout.workspace_path.join("src/lib.rs");
            fs::create_dir_all(code.parent().unwrap()).unwrap();
            fs::write(&code, "pub fn ok() {}\n").unwrap();
            let spec = layout.workspace_path.join(&self.spec_path);
            fs::create_dir_all(spec.parent().unwrap()).unwrap();
            fs::write(&spec, &self.spec_bytes).unwrap();
            assert!(Command::new("git")
                .args(["add", "-A"])
                .current_dir(&layout.workspace_path)
                .status()
                .unwrap()
                .success());
            assert!(Command::new("git")
                .args(["commit", "-m", "implement"])
                .current_dir(&layout.workspace_path)
                .status()
                .unwrap()
                .success());
            let head = git_stdout(&layout.workspace_path, &["rev-parse", "HEAD"]).unwrap();
            let manifest = serde_json::json!({
                "schema_version": 1,
                "status": "completed",
                "head_commit": head,
                "summary": "Adds lib.rs for the approved behavior.",
                "acceptance_criteria": [{
                    "index": 1,
                    "status": "implemented",
                    "evidence": [{
                        "kind": "repository",
                        "reference": "src/lib.rs",
                        "summary": "New library entrypoint."
                    }]
                }],
                "known_limitations": []
            });
            fs::write(
                &layout.output_path,
                serde_json::to_vec_pretty(&manifest).unwrap(),
            )
            .unwrap();
            Ok(StageUsage::default())
        }
    }

    #[tokio::test]
    async fn local_turn_clones_from_bundle_and_assesses() {
        let (repo, base) = init_repo();
        let root = tempdir().unwrap();
        let spec_path = "specs/ISSUE-1/APPROVED-v1.md";
        let spec_bytes = b"---\nsymphony_factory_run: run\n---\n# Spec\n".to_vec();
        let request = ImplementationTurnRequest {
            attempt_id: "att-1".into(),
            workspace_root: root.path().to_path_buf(),
            repo_path: repo.path().to_path_buf(),
            base_commit: base.clone(),
            command: vec!["true".into()],
            prompt: "implement".into(),
            timeout_ms: 60_000,
            model: None,
            harness: TriageHarness::Pi,
            issue: TriageIssueIdentity {
                id: "1".into(),
                identifier: "#1".into(),
                title: "t".into(),
            },
            codex: None,
            approved_spec_markdown: String::from_utf8(spec_bytes.clone()).unwrap(),
            approved_spec_path: spec_path.into(),
            stage_inputs: serde_json::json!({"issue":{"id":"1"}}),
            spawned: None,
            execution_profile: ExecutionProfile::Local,
            existing_workspace: None,
        };
        let harness = CommitHarness {
            spec_path: spec_path.into(),
            spec_bytes: spec_bytes.clone(),
        };
        let result = ImplementationRunner
            .run_local_turn(&request, &harness)
            .await
            .unwrap();
        let head: String = serde_json::from_slice::<serde_json::Value>(&result.output_bytes)
            .unwrap()["head_commit"]
            .as_str()
            .unwrap()
            .into();
        let assessment =
            assess_postconditions(&result.workspace_path, &base, spec_path, &spec_bytes, &head)
                .unwrap();
        assert!(assessment
            .changed_paths
            .iter()
            .any(|path| path == "src/lib.rs"));
        assert!(result.workspace_path.join("src/lib.rs").is_file());
        // Ensure we did not use a shared worktree path.
        assert!(result
            .workspace_path
            .to_string_lossy()
            .contains("attempt-att-1"));
    }

    #[test]
    fn postconditions_reject_spec_only_change() {
        let (repo, base) = init_repo();
        let spec_path = "specs/x/APPROVED-v1.md";
        let spec_bytes = b"# Spec\n";
        let path = repo.path().join(spec_path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, spec_bytes).unwrap();
        assert!(Command::new("git")
            .args(["add", "-A"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-m", "spec only"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success());
        let head = git_stdout(repo.path(), &["rev-parse", "HEAD"]).unwrap();
        let err =
            assess_postconditions(repo.path(), &base, spec_path, spec_bytes, &head).unwrap_err();
        assert!(err.to_string().contains("at least one path other"));
    }

    #[test]
    fn isolated_env_omits_gh_token() {
        let dir = tempdir().unwrap();
        // Ensure parent has GH_TOKEN; builder must not forward it.
        std::env::set_var("GH_TOKEN", "secret-should-not-leak");
        let env = isolated_env_omits_forge_credentials(dir.path(), &dir.path().join("out.json"));
        assert!(!env.contains_key("GH_TOKEN"));
        assert!(!env.contains_key("GITHUB_TOKEN"));
        assert_eq!(
            env.get("HOME").map(String::as_str),
            Some(dir.path().to_str().unwrap())
        );
        std::env::remove_var("GH_TOKEN");
    }

    #[test]
    fn docker_env_builder_omits_forge_and_ssh() {
        let mut parent = HashMap::new();
        parent.insert("GH_TOKEN".into(), "gh".into());
        parent.insert("GITHUB_TOKEN".into(), "ght".into());
        parent.insert("LINEAR_API_KEY".into(), "lin".into());
        parent.insert("SSH_AUTH_SOCK".into(), "/tmp/ssh".into());
        parent.insert("ANTHROPIC_API_KEY".into(), "sk".into());
        parent.insert("PATH".into(), "/usr/bin".into());
        let env =
            docker_credential_isolated_env("/home/worker", "/out/result.json", Some("m"), &parent);
        assert_no_forge_credentials(&env).unwrap();
        assert!(env
            .iter()
            .any(|(k, v)| k == "ANTHROPIC_API_KEY" && v == "sk"));
        assert!(!env.iter().any(|(k, _)| k == "GH_TOKEN"));
        assert!(!env.iter().any(|(k, _)| k == "SSH_AUTH_SOCK"));
    }

    #[test]
    fn create_base_bundle_roundtrip_for_runner() {
        let (repo, head) = init_repo();
        let tmp = tempdir().unwrap();
        let bundle = tmp.path().join("base.bundle");
        create_base_bundle(repo.path(), &head, &bundle).unwrap();
        verify_bundle(&bundle).unwrap();
    }
}
