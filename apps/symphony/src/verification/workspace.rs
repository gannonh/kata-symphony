//! Exact-head disposable workspaces for the verification stage.
//!
//! The trusted controller fetches `refs/pull/<number>/head` with
//! subprocess-scoped authentication, verifies the fetched SHA equals the A4
//! reviewed head, and creates a credential-free bundle clone. Commands and the
//! verifier work only from that clone — there is no authenticated remote.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use base64::Engine;

use crate::error::{Result, SymphonyError};
use crate::implementation::bundle::{clone_from_bundle, create_base_bundle};

pub const VERIFICATION_ATTEMPT_DIR_PREFIX: &str = "verification-";

/// Layout of one disposable verification attempt.
#[derive(Debug, Clone)]
pub struct VerificationWorkspace {
    pub attempt_root: PathBuf,
    pub workspace_path: PathBuf,
    pub evidence_dir: PathBuf,
    pub home_dir: PathBuf,
    pub head_sha: String,
}

/// Fetch the live pull head with subprocess-scoped auth and verify it equals
/// the reviewed head. Returns the resolved commit SHA.
pub async fn fetch_pull_head_verified(
    repo_path: &Path,
    pr_number: u64,
    expected_head_sha: &str,
    github_token: Option<&str>,
    ref_name: &str,
) -> Result<String> {
    let mut command = Command::new("git");
    command
        .args([
            "fetch",
            "--no-tags",
            "origin",
            &format!("refs/pull/{pr_number}/head:refs/{ref_name}"),
        ])
        .current_dir(repo_path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "");
    if let Some(token) = github_token
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        let credentials =
            base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
        command
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.extraHeader")
            .env(
                "GIT_CONFIG_VALUE_0",
                format!("Authorization: Basic {credentials}"),
            );
    }

    let started = Instant::now();
    let mut child = command.spawn().map_err(|error| {
        SymphonyError::TriageError(format!(
            "failed fetching pull head for #{pr_number}: {error}"
        ))
    })?;
    let status = loop {
        match child
            .try_wait()
            .map_err(|error| SymphonyError::TriageError(format!("git fetch wait: {error}")))?
        {
            Some(status) => break status,
            None if started.elapsed() >= Duration::from_secs(120) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SymphonyError::TriageError(format!(
                    "git fetch of pull head #{pr_number} timed out"
                )));
            }
            None => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    };
    if !status.success() {
        return Err(SymphonyError::TriageError(format!(
            "git fetch of pull head #{pr_number} failed with status {status}"
        )));
    }

    let resolved = git_stdout(
        repo_path,
        &[
            "rev-parse",
            "--verify",
            &format!("refs/{ref_name}^{{commit}}"),
        ],
    )?;
    if resolved != expected_head_sha {
        return Err(SymphonyError::TriageError(format!(
            "fetched pull head {resolved} does not equal the A4 reviewed head {expected_head_sha}"
        )));
    }
    Ok(resolved)
}

/// Prepare a fresh attempt layout under `workspace_root` and populate it with
/// a credential-free clone of the reviewed head.
pub async fn prepare_verification_workspace(
    repo_path: &Path,
    workspace_root: &Path,
    attempt_id: &str,
    head_sha: &str,
) -> Result<VerificationWorkspace> {
    let attempt_root =
        workspace_root.join(format!("{VERIFICATION_ATTEMPT_DIR_PREFIX}{attempt_id}"));
    let workspace_path = attempt_root.join("workspace");
    let evidence_dir = attempt_root.join("evidence");
    let home_dir = attempt_root.join("home");

    if attempt_root.exists() {
        return Err(SymphonyError::TriageError(format!(
            "verification attempt root already exists: {}",
            attempt_root.display()
        )));
    }
    fs::create_dir_all(&workspace_path).map_err(|error| {
        SymphonyError::TriageError(format!(
            "failed creating verification workspace {}: {error}",
            workspace_path.display()
        ))
    })?;
    fs::create_dir_all(&evidence_dir).map_err(|error| {
        SymphonyError::TriageError(format!(
            "failed creating verification evidence dir {}: {error}",
            evidence_dir.display()
        ))
    })?;
    fs::create_dir_all(&home_dir).map_err(|error| {
        SymphonyError::TriageError(format!(
            "failed creating verification home {}: {error}",
            home_dir.display()
        ))
    })?;

    let bundle_path = attempt_root.join("base.bundle");
    if let Err(error) = create_base_bundle(repo_path, head_sha, &bundle_path) {
        let _ = fs::remove_dir_all(&attempt_root);
        return Err(error);
    }
    let _ = fs::remove_dir_all(&workspace_path);
    if let Err(error) = clone_from_bundle(&bundle_path, &workspace_path) {
        let _ = fs::remove_dir_all(&attempt_root);
        return Err(error);
    }
    let actual = git_stdout(&workspace_path, &["rev-parse", "HEAD"])?;
    if actual != head_sha {
        let _ = fs::remove_dir_all(&attempt_root);
        return Err(SymphonyError::TriageError(format!(
            "workspace HEAD {actual} does not equal reviewed head {head_sha}"
        )));
    }
    // A bundle clone has no remote; strip any inherited configuration that
    // could carry credentials.
    let _ = git_stdout(&workspace_path, &["remote", "remove", "origin"]);

    Ok(VerificationWorkspace {
        attempt_root,
        workspace_path,
        evidence_dir,
        home_dir,
        head_sha: head_sha.to_string(),
    })
}

/// Verify the reviewed head, committed tree, and tracked files are unchanged
/// from the pinned revision after commands ran.
pub fn verify_workspace_unchanged(workspace: &Path, expected_head: &str) -> Result<()> {
    let head = git_stdout(workspace, &["rev-parse", "HEAD"])?;
    if head != expected_head {
        return Err(SymphonyError::TriageError(format!(
            "workspace HEAD {head} changed; expected {expected_head}"
        )));
    }
    let tree = git_stdout(workspace, &["rev-parse", "HEAD^{tree}"])?;
    let status = git_stdout(
        workspace,
        &["status", "--porcelain", "--untracked-files=normal"],
    )?;
    if !status.trim().is_empty() {
        return Err(SymphonyError::TriageError(format!(
            "workspace is not clean after verification commands:\n{status}"
        )));
    }
    let tracked = git_stdout(workspace, &["ls-files"])?;
    let tracked_count = tracked
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let _ = tree;
    if tracked_count == 0 {
        return Err(SymphonyError::TriageError(
            "workspace has no tracked files; cannot attest the reviewed tree".to_string(),
        ));
    }
    Ok(())
}

/// The attempt root for cleanup, accepting only `verification-<attempt_id>`
/// directories directly beneath the configured workspace root.
pub fn attempt_root_for_cleanup(
    workspace_path: &Path,
    workspace_root: &Path,
    attempt_id: &str,
) -> Option<PathBuf> {
    let root = workspace_path.parent()?;
    let expected_name = format!("{VERIFICATION_ATTEMPT_DIR_PREFIX}{attempt_id}");
    if root.file_name()?.to_str()? != expected_name {
        return None;
    }
    let parent = root.parent()?;
    paths_equal(parent, workspace_root).then(|| root.to_path_buf())
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
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

    #[tokio::test]
    async fn prepares_a_credential_free_clone_of_the_reviewed_head() {
        let (repo, head) = init_repo();
        let root = tempdir().unwrap();
        let workspace = prepare_verification_workspace(repo.path(), root.path(), "att-1", &head)
            .await
            .unwrap();

        assert_eq!(workspace.head_sha, head);
        assert!(workspace.workspace_path.join("README.md").is_file());
        assert!(workspace.evidence_dir.is_dir());
        // No authenticated remote survives into the disposable workspace.
        let remotes = git_stdout(&workspace.workspace_path, &["remote", "-v"]).unwrap();
        assert!(remotes.is_empty(), "bundle clone must have no remote");
        let config = fs::read_to_string(workspace.workspace_path.join(".git/config")).unwrap();
        assert!(
            !config.contains("extraHeader"),
            "no credential config may leak"
        );
        assert!(
            !config.contains("http."),
            "no http credential config may leak"
        );
    }

    #[tokio::test]
    async fn rejects_a_head_mismatch() {
        let (repo, head) = init_repo();
        let root = tempdir().unwrap();
        let error = prepare_verification_workspace(
            repo.path(),
            root.path(),
            "att-1",
            "0000000000000000000000000000000000000000",
        )
        .await
        .unwrap_err();
        assert!(
            !error.to_string().is_empty() && !root.path().join("verification-att-1").exists(),
            "a failed preparation must clean up its attempt root: {error}"
        );
        let _ = head;
    }

    #[test]
    fn cleanup_root_accepts_only_runner_created_attempt_dirs() {
        let workspace_root = Path::new("/srv/workspaces");
        assert_eq!(
            attempt_root_for_cleanup(
                Path::new("/srv/workspaces/verification-abc/workspace"),
                workspace_root,
                "abc",
            ),
            Some(PathBuf::from("/srv/workspaces/verification-abc"))
        );
        assert_eq!(
            attempt_root_for_cleanup(
                Path::new("/home/user/verification-project/workspace"),
                workspace_root,
                "project",
            ),
            None
        );
        assert_eq!(
            attempt_root_for_cleanup(
                Path::new("/srv/workspaces/verification-other/workspace"),
                workspace_root,
                "abc",
            ),
            None
        );
        assert_eq!(
            attempt_root_for_cleanup(Path::new("/srv/workspaces"), workspace_root, "abc"),
            None
        );
    }

    #[test]
    fn unchanged_workspace_passes_and_dirty_tree_fails() {
        let (repo, head) = init_repo();
        verify_workspace_unchanged(repo.path(), &head).unwrap();
        fs::write(repo.path().join("dirty.txt"), "x\n").unwrap();
        let error = verify_workspace_unchanged(repo.path(), &head).unwrap_err();
        assert!(error.to_string().contains("not clean"));
        fs::remove_file(repo.path().join("dirty.txt")).unwrap();
        fs::write(repo.path().join("README.md"), "changed\n").unwrap();
        let error = verify_workspace_unchanged(repo.path(), &head).unwrap_err();
        assert!(error.to_string().contains("not clean"));
    }

    #[test]
    fn changed_head_fails_unchanged_check() {
        let (repo, head) = init_repo();
        fs::write(repo.path().join("README.md"), "changed\n").unwrap();
        assert!(Command::new("git")
            .args(["add", "README.md"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-m", "change"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success());
        let error = verify_workspace_unchanged(repo.path(), &head).unwrap_err();
        assert!(error.to_string().contains("changed"));
    }
}
