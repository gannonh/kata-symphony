//! Trusted-host branch expected-projection for A3 automatic publication.
//!
//! Force push is prohibited. Workers never receive forge credentials; only this
//! publisher path pushes to a configured remote.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use crate::error::{Result, SymphonyError};
use crate::implementation::bundle::{blob_path, sha256_file, verify_bundle};
use crate::path_safety::sanitize_identifier;

pub const BRANCH_PUSHED_STEP: &str = "branch_pushed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchProjectionDecision {
    /// Remote branch is absent — push the desired head (no force).
    PushAbsent,
    /// Remote already points at the desired head — continue.
    AlreadyDesired,
    /// Remote matches the recorded expected SHA and is a fast-forward ancestor of desired.
    FastForwardPush,
    /// Unrelated remote SHA — human conflict; never force.
    HumanConflict { remote_sha: String },
}

/// Pure projection decision table (unit-tested).
pub fn decide_branch_projection(
    remote_sha: Option<&str>,
    desired_head: &str,
    expected_remote_sha: Option<&str>,
    remote_is_ancestor_of_desired: bool,
) -> BranchProjectionDecision {
    match remote_sha {
        None => BranchProjectionDecision::PushAbsent,
        Some(sha) if sha == desired_head => BranchProjectionDecision::AlreadyDesired,
        Some(sha) if expected_remote_sha == Some(sha) && remote_is_ancestor_of_desired => {
            BranchProjectionDecision::FastForwardPush
        }
        Some(sha) => BranchProjectionDecision::HumanConflict {
            remote_sha: sha.to_string(),
        },
    }
}

/// Deterministic publication branch: `{branch_prefix}/{sanitize(issue_identifier)}`.
pub fn branch_name_for_issue(branch_prefix: &str, issue_identifier: &str) -> String {
    format!(
        "{}/{}",
        branch_prefix.trim_matches('/'),
        sanitize_identifier(issue_identifier)
    )
}

#[derive(Debug, Clone)]
pub struct BranchPublishRequest<'a> {
    pub artifacts_dir: &'a Path,
    pub bundle_sha256: &'a str,
    pub bundle_bytes_len: u64,
    pub desired_head: &'a str,
    pub expected_remote_sha: Option<&'a str>,
    pub branch_name: &'a str,
    pub base_branch: &'a str,
    /// Trusted remote URL (local bare path or `https://…` with credentials for live use).
    pub remote_url: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchPublishOutcome {
    pub decision: BranchProjectionDecision,
    pub remote_sha_before: Option<String>,
    pub remote_sha_after: String,
    pub pushed: bool,
}

/// Load the content-addressed blob, re-verify digest + git-bundle, then return its path.
pub fn load_and_reverify_bundle(
    artifacts_dir: &Path,
    sha256: &str,
    expected_bytes: u64,
) -> Result<PathBuf> {
    let path = blob_path(artifacts_dir, sha256);
    if !path.is_file() {
        return Err(SymphonyError::StorageError(format!(
            "bundle blob missing at {}",
            path.display()
        )));
    }
    let actual = sha256_file(&path)?;
    if actual != sha256 {
        return Err(SymphonyError::StorageError(format!(
            "bundle digest mismatch at {}: expected {sha256}, got {actual}",
            path.display()
        )));
    }
    let meta = fs::metadata(&path).map_err(|error| {
        SymphonyError::StorageError(format!("failed stating {}: {error}", path.display()))
    })?;
    if meta.len() != expected_bytes {
        return Err(SymphonyError::StorageError(format!(
            "bundle size mismatch at {}: {} != {expected_bytes}",
            path.display(),
            meta.len()
        )));
    }
    verify_bundle(&path)?;
    Ok(path)
}

/// Reconstruct a temp repo from the trusted remote base, import the verified
/// result bundle, and reconcile the remote branch without force.
pub fn publish_branch(request: &BranchPublishRequest<'_>) -> Result<BranchPublishOutcome> {
    let bundle_path = load_and_reverify_bundle(
        request.artifacts_dir,
        request.bundle_sha256,
        request.bundle_bytes_len,
    )?;

    let temp = TempDir::new().map_err(|error| {
        SymphonyError::StorageError(format!("failed creating publication temp dir: {error}"))
    })?;
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).map_err(|error| {
        SymphonyError::StorageError(format!("failed creating publication repo: {error}"))
    })?;

    run_git(&repo, &["init"])?;
    run_git(&repo, &["remote", "add", "origin", request.remote_url])?;
    // Fetch base branch history so the result bundle can attach.
    let fetch_base = Command::new("git")
        .args([
            "fetch",
            "origin",
            &format!(
                "+refs/heads/{0}:refs/remotes/origin/{0}",
                request.base_branch
            ),
        ])
        .current_dir(&repo)
        .output()
        .map_err(|error| SymphonyError::TriageError(format!("git fetch base failed: {error}")))?;
    if !fetch_base.status.success() {
        return Err(SymphonyError::TriageError(format!(
            "git fetch base `{}` failed: {}",
            request.base_branch,
            String::from_utf8_lossy(&fetch_base.stderr).trim()
        )));
    }

    // Import result bundle onto the temp repo at the desired head.
    let fetch_bundle = Command::new("git")
        .args([
            "fetch",
            bundle_path
                .to_str()
                .ok_or_else(|| SymphonyError::TriageError("bundle path is not UTF-8".into()))?,
            &format!("HEAD:refs/heads/{}", request.branch_name),
        ])
        .current_dir(&repo)
        .output()
        .map_err(|error| {
            SymphonyError::TriageError(format!("git fetch result bundle failed: {error}"))
        })?;
    if !fetch_bundle.status.success() {
        // Fallback: fetch as bundle-head then create the publication branch.
        let alt = Command::new("git")
            .args([
                "fetch",
                bundle_path.to_str().unwrap_or_default(),
                "HEAD:refs/heads/bundle-head",
            ])
            .current_dir(&repo)
            .output()
            .map_err(|error| {
                SymphonyError::TriageError(format!("git fetch result bundle (alt) failed: {error}"))
            })?;
        if !alt.status.success() {
            return Err(SymphonyError::TriageError(format!(
                "git import result bundle failed: {}",
                String::from_utf8_lossy(&fetch_bundle.stderr).trim()
            )));
        }
        run_git(&repo, &["branch", "-f", request.branch_name, "bundle-head"])?;
    }

    // Ensure local branch tip is the desired head.
    let local_head = rev_parse(&repo, &format!("refs/heads/{}", request.branch_name))?;
    if local_head != request.desired_head {
        // Move branch to desired commit if the bundle tip differs in naming only.
        let has_desired = Command::new("git")
            .args([
                "cat-file",
                "-e",
                &format!("{}^{{commit}}", request.desired_head),
            ])
            .current_dir(&repo)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !has_desired {
            return Err(SymphonyError::TriageError(format!(
                "desired head {} not present after bundle import (local={local_head})",
                request.desired_head
            )));
        }
        run_git(
            &repo,
            &["branch", "-f", request.branch_name, request.desired_head],
        )?;
    }

    let remote_sha_before = observe_remote_branch(request.remote_url, request.branch_name)?;
    let ancestor = match remote_sha_before.as_deref() {
        Some(remote) => is_ancestor(&repo, remote, request.desired_head)?,
        None => false,
    };
    let decision = decide_branch_projection(
        remote_sha_before.as_deref(),
        request.desired_head,
        request.expected_remote_sha,
        ancestor,
    );

    match &decision {
        BranchProjectionDecision::HumanConflict { remote_sha } => {
            return Err(SymphonyError::TriageError(format!(
                "implementation branch conflict on {}: remote={remote_sha} desired={} expected={:?}",
                request.branch_name,
                request.desired_head,
                request.expected_remote_sha
            )));
        }
        BranchProjectionDecision::AlreadyDesired => {
            return Ok(BranchPublishOutcome {
                decision,
                remote_sha_before,
                remote_sha_after: request.desired_head.to_string(),
                pushed: false,
            });
        }
        BranchProjectionDecision::PushAbsent | BranchProjectionDecision::FastForwardPush => {
            // Non-force push only.
            let push = Command::new("git")
                .args([
                    "push",
                    "origin",
                    &format!("refs/heads/{0}:refs/heads/{0}", request.branch_name),
                ])
                .current_dir(&repo)
                .output()
                .map_err(|error| SymphonyError::TriageError(format!("git push failed: {error}")))?;
            if !push.status.success() {
                return Err(SymphonyError::TriageError(format!(
                    "git push (no-force) failed: {}",
                    String::from_utf8_lossy(&push.stderr).trim()
                )));
            }
        }
    }

    let remote_sha_after = observe_remote_branch(request.remote_url, request.branch_name)?
        .ok_or_else(|| {
            SymphonyError::TriageError(format!(
                "remote branch {} missing after push",
                request.branch_name
            ))
        })?;
    if remote_sha_after != request.desired_head {
        return Err(SymphonyError::TriageError(format!(
            "remote branch {} after push is {remote_sha_after}, expected {}",
            request.branch_name, request.desired_head
        )));
    }

    Ok(BranchPublishOutcome {
        decision,
        remote_sha_before,
        remote_sha_after,
        pushed: true,
    })
}

fn observe_remote_branch(remote_url: &str, branch_name: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .args([
            "ls-remote",
            remote_url,
            &format!("refs/heads/{branch_name}"),
        ])
        .output()
        .map_err(|error| SymphonyError::TriageError(format!("git ls-remote failed: {error}")))?;
    if !output.status.success() {
        return Err(SymphonyError::TriageError(format!(
            "git ls-remote failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return Ok(None);
    }
    let sha = line.split_whitespace().next().unwrap_or("").trim();
    if sha.is_empty() {
        Ok(None)
    } else {
        Ok(Some(sha.to_string()))
    }
}

fn is_ancestor(repo: &Path, maybe_ancestor: &str, tip: &str) -> Result<bool> {
    let status = Command::new("git")
        .args(["merge-base", "--is-ancestor", maybe_ancestor, tip])
        .current_dir(repo)
        .status()
        .map_err(|error| {
            SymphonyError::TriageError(format!("git merge-base --is-ancestor failed: {error}"))
        })?;
    Ok(status.success())
}

fn rev_parse(repo: &Path, rev: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(repo)
        .output()
        .map_err(|error| SymphonyError::TriageError(format!("git rev-parse failed: {error}")))?;
    if !output.status.success() {
        return Err(SymphonyError::TriageError(format!(
            "git rev-parse {rev} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementation::bundle::{create_result_bundle, store_blob_atomic, BlobSource};
    use std::process::Command;
    use tempfile::tempdir;

    #[test]
    fn projection_table_covers_four_cases() {
        assert_eq!(
            decide_branch_projection(None, "aaa", None, false),
            BranchProjectionDecision::PushAbsent
        );
        assert_eq!(
            decide_branch_projection(Some("aaa"), "aaa", Some("bbb"), false),
            BranchProjectionDecision::AlreadyDesired
        );
        assert_eq!(
            decide_branch_projection(Some("bbb"), "aaa", Some("bbb"), true),
            BranchProjectionDecision::FastForwardPush
        );
        assert_eq!(
            decide_branch_projection(Some("ccc"), "aaa", Some("bbb"), false),
            BranchProjectionDecision::HumanConflict {
                remote_sha: "ccc".into()
            }
        );
        // Expected match but not fast-forwardable → conflict.
        assert_eq!(
            decide_branch_projection(Some("bbb"), "aaa", Some("bbb"), false),
            BranchProjectionDecision::HumanConflict {
                remote_sha: "bbb".into()
            }
        );
    }

    #[test]
    fn branch_name_uses_prefix_and_sanitized_identifier() {
        assert_eq!(branch_name_for_issue("symphony", "#42"), "symphony/_42");
        assert_eq!(
            branch_name_for_issue("symphony/", "KATA-123"),
            "symphony/KATA-123"
        );
    }

    fn init_repo(path: &Path) -> String {
        run_git(path, &["init"]).unwrap();
        run_git(path, &["config", "user.email", "a3@example.com"]).unwrap();
        run_git(path, &["config", "user.name", "A3 Test"]).unwrap();
        // Prefer "main" for base branch naming in tests.
        let _ = Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(path)
            .status();
        fs::write(path.join("README.md"), "base\n").unwrap();
        run_git(path, &["add", "README.md"]).unwrap();
        run_git(path, &["commit", "-m", "init"]).unwrap();
        rev_parse(path, "HEAD").unwrap()
    }

    fn commit_file(repo: &Path, name: &str, contents: &str, message: &str) -> String {
        fs::write(repo.join(name), contents).unwrap();
        run_git(repo, &["add", name]).unwrap();
        run_git(repo, &["commit", "-m", message]).unwrap();
        rev_parse(repo, "HEAD").unwrap()
    }

    fn make_bare_remote(seed: &Path) -> TempDir {
        let bare = tempdir().unwrap();
        run_git(bare.path(), &["init", "--bare"]).unwrap();
        run_git(
            seed,
            &["remote", "add", "origin", bare.path().to_str().unwrap()],
        )
        .unwrap();
        run_git(seed, &["push", "-u", "origin", "main"]).unwrap();
        bare
    }

    fn prepare_bundle(workspace: &Path, base: &str, head: &str, arts: &Path) -> (String, u64) {
        let bundle_file = workspace.join("result.bundle");
        create_result_bundle(workspace, base, head, &bundle_file).unwrap();
        store_blob_atomic(arts, BlobSource::Path(&bundle_file), 50 * 1024 * 1024).unwrap()
    }

    #[test]
    fn bare_remote_absent_push() {
        let seed = tempdir().unwrap();
        let base = init_repo(seed.path());
        let bare = make_bare_remote(seed.path());

        let work = tempdir().unwrap();
        run_git(work.path(), &["clone", bare.path().to_str().unwrap(), "."]).unwrap();
        run_git(work.path(), &["config", "user.email", "a3@example.com"]).unwrap();
        run_git(work.path(), &["config", "user.name", "A3 Test"]).unwrap();
        let head = commit_file(work.path(), "feature.rs", "impl\n", "feature");

        let arts = tempdir().unwrap();
        let (sha, bytes) = prepare_bundle(work.path(), &base, &head, arts.path());

        let outcome = publish_branch(&BranchPublishRequest {
            artifacts_dir: arts.path(),
            bundle_sha256: &sha,
            bundle_bytes_len: bytes,
            desired_head: &head,
            expected_remote_sha: None,
            branch_name: "symphony/42",
            base_branch: "main",
            remote_url: bare.path().to_str().unwrap(),
        })
        .unwrap();
        assert!(outcome.pushed);
        assert_eq!(outcome.decision, BranchProjectionDecision::PushAbsent);
        assert_eq!(outcome.remote_sha_after, head);
        assert_eq!(
            observe_remote_branch(bare.path().to_str().unwrap(), "symphony/42").unwrap(),
            Some(head)
        );
    }

    #[test]
    fn bare_remote_already_desired() {
        let seed = tempdir().unwrap();
        let base = init_repo(seed.path());
        let bare = make_bare_remote(seed.path());

        let work = tempdir().unwrap();
        run_git(work.path(), &["clone", bare.path().to_str().unwrap(), "."]).unwrap();
        run_git(work.path(), &["config", "user.email", "a3@example.com"]).unwrap();
        run_git(work.path(), &["config", "user.name", "A3 Test"]).unwrap();
        let head = commit_file(work.path(), "feature.rs", "impl\n", "feature");
        run_git(
            work.path(),
            &["push", "origin", &format!("{head}:refs/heads/symphony/42")],
        )
        .unwrap();

        let arts = tempdir().unwrap();
        let (sha, bytes) = prepare_bundle(work.path(), &base, &head, arts.path());

        let outcome = publish_branch(&BranchPublishRequest {
            artifacts_dir: arts.path(),
            bundle_sha256: &sha,
            bundle_bytes_len: bytes,
            desired_head: &head,
            expected_remote_sha: Some(&head),
            branch_name: "symphony/42",
            base_branch: "main",
            remote_url: bare.path().to_str().unwrap(),
        })
        .unwrap();
        assert!(!outcome.pushed);
        assert_eq!(outcome.decision, BranchProjectionDecision::AlreadyDesired);
    }

    #[test]
    fn bare_remote_fast_forward() {
        let seed = tempdir().unwrap();
        let base = init_repo(seed.path());
        let bare = make_bare_remote(seed.path());

        let work = tempdir().unwrap();
        run_git(work.path(), &["clone", bare.path().to_str().unwrap(), "."]).unwrap();
        run_git(work.path(), &["config", "user.email", "a3@example.com"]).unwrap();
        run_git(work.path(), &["config", "user.name", "A3 Test"]).unwrap();
        let mid = commit_file(work.path(), "feature.rs", "v1\n", "v1");
        run_git(
            work.path(),
            &["push", "origin", &format!("{mid}:refs/heads/symphony/42")],
        )
        .unwrap();
        let head = commit_file(work.path(), "feature.rs", "v2\n", "v2");

        let arts = tempdir().unwrap();
        let (sha, bytes) = prepare_bundle(work.path(), &base, &head, arts.path());

        let outcome = publish_branch(&BranchPublishRequest {
            artifacts_dir: arts.path(),
            bundle_sha256: &sha,
            bundle_bytes_len: bytes,
            desired_head: &head,
            expected_remote_sha: Some(&mid),
            branch_name: "symphony/42",
            base_branch: "main",
            remote_url: bare.path().to_str().unwrap(),
        })
        .unwrap();
        assert!(outcome.pushed);
        assert_eq!(outcome.decision, BranchProjectionDecision::FastForwardPush);
        assert_eq!(outcome.remote_sha_after, head);
    }

    #[test]
    fn bare_remote_conflict_refuses_force() {
        let seed = tempdir().unwrap();
        let base = init_repo(seed.path());
        let bare = make_bare_remote(seed.path());

        let work = tempdir().unwrap();
        run_git(work.path(), &["clone", bare.path().to_str().unwrap(), "."]).unwrap();
        run_git(work.path(), &["config", "user.email", "a3@example.com"]).unwrap();
        run_git(work.path(), &["config", "user.name", "A3 Test"]).unwrap();
        let ours = commit_file(work.path(), "feature.rs", "ours\n", "ours");

        // Divergent remote tip.
        let other = tempdir().unwrap();
        run_git(other.path(), &["clone", bare.path().to_str().unwrap(), "."]).unwrap();
        run_git(other.path(), &["config", "user.email", "a3@example.com"]).unwrap();
        run_git(other.path(), &["config", "user.name", "A3 Test"]).unwrap();
        let foreign = commit_file(other.path(), "other.rs", "foreign\n", "foreign");
        run_git(
            other.path(),
            &[
                "push",
                "origin",
                &format!("{foreign}:refs/heads/symphony/42"),
            ],
        )
        .unwrap();

        let arts = tempdir().unwrap();
        let (sha, bytes) = prepare_bundle(work.path(), &base, &ours, arts.path());

        let err = publish_branch(&BranchPublishRequest {
            artifacts_dir: arts.path(),
            bundle_sha256: &sha,
            bundle_bytes_len: bytes,
            desired_head: &ours,
            expected_remote_sha: Some(&base),
            branch_name: "symphony/42",
            base_branch: "main",
            remote_url: bare.path().to_str().unwrap(),
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("conflict"),
            "expected conflict, got {err}"
        );
        // Remote unchanged.
        assert_eq!(
            observe_remote_branch(bare.path().to_str().unwrap(), "symphony/42").unwrap(),
            Some(foreign)
        );
    }
}
