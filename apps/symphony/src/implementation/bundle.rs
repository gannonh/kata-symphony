//! Content-addressed Git bundle create/verify/import.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use tempfile::tempdir;
use uuid::Uuid;

use crate::error::{Result, SymphonyError};

const TEMP_PREFIX: &str = ".symphony-bundle-tmp-";

fn unique_temp_name(sha256: &str) -> String {
    format!(
        "{TEMP_PREFIX}{sha256}.{}.{}",
        std::process::id(),
        Uuid::new_v4()
    )
}

/// Derive the content-addressed artifacts directory beside the factory database.
pub fn artifacts_dir(storage_path: &Path) -> PathBuf {
    let mut name = storage_path
        .file_name()
        .map(|value| value.to_os_string())
        .unwrap_or_else(|| "factory.db".into());
    name.push(".artifacts");
    storage_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(name)
}

/// Layout: `{artifacts_dir}/sha256/<first2>/<full-digest>`.
pub fn blob_path(artifacts_dir: &Path, sha256: &str) -> PathBuf {
    let prefix = sha256.get(..2).unwrap_or("00");
    artifacts_dir.join("sha256").join(prefix).join(sha256)
}

/// Create a verified base bundle containing `commit` from `repo`.
pub fn create_base_bundle(repo: &Path, commit: &str, dest: &Path) -> Result<()> {
    let dest = absolute_output_path(dest)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            SymphonyError::StorageError(format!(
                "failed creating base bundle parent {}: {error}",
                parent.display()
            ))
        })?;
    }
    // Raw SHAs produce empty bundles; a detached worktree tip publishes `HEAD`.
    let worktree = tempfile::tempdir().map_err(|error| {
        SymphonyError::StorageError(format!("failed creating base worktree dir: {error}"))
    })?;
    run_git(
        repo,
        &[
            "worktree",
            "add",
            "--detach",
            worktree.path().to_str().ok_or_else(|| {
                SymphonyError::StorageError("base worktree path is not UTF-8".to_string())
            })?,
            commit,
        ],
    )?;
    let output = Command::new("git")
        .args(["bundle", "create"])
        .arg(&dest)
        .arg("HEAD")
        .current_dir(worktree.path())
        .output();
    let _ = Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(worktree.path())
        .current_dir(repo)
        .status();
    let output = output.map_err(|error| {
        SymphonyError::TriageError(format!("git bundle create (base) failed: {error}"))
    })?;
    if !output.status.success() {
        return Err(SymphonyError::TriageError(format!(
            "git bundle create (base) failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    verify_bundle(&dest)?;
    Ok(())
}

/// Create a verified result bundle for `base..head` from `workspace`.
///
/// The bundle is SELF-CONTAINED: the base commit is included as an object, so
/// `git bundle verify` succeeds without a prerequisite repository. Thin
/// `base..HEAD` bundles fail standalone verification, which the A3 publication
/// path requires when it re-verifies the stored blob.
pub fn create_result_bundle(workspace: &Path, base: &str, head: &str, dest: &Path) -> Result<()> {
    let dest = absolute_output_path(dest)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            SymphonyError::StorageError(format!(
                "failed creating result bundle parent {}: {error}",
                parent.display()
            ))
        })?;
    }
    let worktree = tempfile::tempdir().map_err(|error| {
        SymphonyError::StorageError(format!("failed creating result worktree dir: {error}"))
    })?;
    run_git(
        workspace,
        &[
            "worktree",
            "add",
            "--detach",
            worktree.path().to_str().ok_or_else(|| {
                SymphonyError::StorageError("result worktree path is not UTF-8".to_string())
            })?,
            head,
        ],
    )?;
    let output = Command::new("git")
        .args(["bundle", "create"])
        .arg(&dest)
        .arg(base)
        .arg("HEAD")
        .current_dir(worktree.path())
        .output();
    let _ = Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(worktree.path())
        .current_dir(workspace)
        .status();
    let output = output.map_err(|error| {
        SymphonyError::TriageError(format!("git bundle create (result) failed: {error}"))
    })?;
    if !output.status.success() {
        return Err(SymphonyError::TriageError(format!(
            "git bundle create (result) failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    // Thin result bundles require the workspace (which has `base`) for verify.
    verify_bundle_in(workspace, &dest)?;
    Ok(())
}

pub fn verify_bundle(path: &Path) -> Result<()> {
    // `git bundle verify` demands a repository context even for complete
    // bundles. Verify against a neutral empty repository so the runtime
    // working directory never matters (the tests only passed before because
    // the crate directory sits inside a git checkout).
    let context = tempdir().map_err(|error| {
        SymphonyError::StorageError(format!("failed creating bundle verify context: {error}"))
    })?;
    let context_git_dir = context.path().join(".git");
    let init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(context.path())
        .output()
        .map_err(|error| {
            SymphonyError::TriageError(format!("git init for bundle verify failed: {error}"))
        })?;
    if !init.status.success() {
        return Err(SymphonyError::TriageError(format!(
            "git init for bundle verify failed: {}",
            String::from_utf8_lossy(&init.stderr).trim()
        )));
    }
    let output = Command::new("git")
        .arg("--git-dir")
        .arg(&context_git_dir)
        .args(["bundle", "verify"])
        .arg(path)
        .output()
        .map_err(|error| {
            SymphonyError::TriageError(format!("git bundle verify failed: {error}"))
        })?;
    if !output.status.success() {
        return Err(SymphonyError::TriageError(format!(
            "git bundle verify failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

pub fn verify_bundle_in(repo: &Path, path: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["bundle", "verify"])
        .arg(path)
        .current_dir(repo)
        .output()
        .map_err(|error| {
            SymphonyError::TriageError(format!("git bundle verify failed: {error}"))
        })?;
    if !output.status.success() {
        return Err(SymphonyError::TriageError(format!(
            "git bundle verify failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|error| {
        SymphonyError::StorageError(format!("failed opening {}: {error}", path.display()))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            SymphonyError::StorageError(format!("failed reading {}: {error}", path.display()))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Atomically store bytes (or a file) under content-addressed layout.
///
/// Returns `(sha256, bytes_len)`. The staged copy is hashed after the source
/// copy and its digest/size are compared with the intended identity before the
/// rename, so a source that mutates between hashing and storing can never land
/// beneath the wrong digest. Existing digests are re-verified before reuse.
pub fn store_blob_atomic(
    artifacts_dir: &Path,
    source: BlobSource<'_>,
    max_bytes: u64,
) -> Result<(String, u64)> {
    fs::create_dir_all(artifacts_dir).map_err(|error| {
        SymphonyError::StorageError(format!(
            "failed creating artifacts dir {}: {error}",
            artifacts_dir.display()
        ))
    })?;

    let (sha256, bytes_len, temp_path) = match source {
        BlobSource::Bytes(bytes) => {
            let bytes_len = bytes.len() as u64;
            if bytes_len > max_bytes {
                return Err(SymphonyError::StorageError(format!(
                    "bundle is {bytes_len} bytes; max_bundle_bytes is {max_bytes}"
                )));
            }
            let sha256 = hex::encode(Sha256::digest(bytes));
            let staged = artifacts_dir.join(unique_temp_name(&sha256));
            {
                let mut file = File::create(&staged).map_err(|error| {
                    SymphonyError::StorageError(format!("failed writing temp blob: {error}"))
                })?;
                file.write_all(bytes).map_err(|error| {
                    SymphonyError::StorageError(format!("failed writing temp blob: {error}"))
                })?;
                file.sync_all().map_err(|error| {
                    SymphonyError::StorageError(format!("failed syncing temp blob: {error}"))
                })?;
            }
            (sha256, bytes_len, staged)
        }
        BlobSource::Path(path) => {
            let meta = fs::metadata(path).map_err(|error| {
                SymphonyError::StorageError(format!("failed stating {}: {error}", path.display()))
            })?;
            let bytes_len = meta.len();
            if bytes_len > max_bytes {
                return Err(SymphonyError::StorageError(format!(
                    "bundle is {bytes_len} bytes; max_bundle_bytes is {max_bytes}"
                )));
            }
            let sha256 = sha256_file(path)?;
            let staged = artifacts_dir.join(unique_temp_name(&sha256));
            fs::copy(path, &staged).map_err(|error| {
                SymphonyError::StorageError(format!(
                    "failed copying blob into temp {}: {error}",
                    staged.display()
                ))
            })?;
            let file = File::open(&staged).map_err(|error| {
                SymphonyError::StorageError(format!("failed opening staged blob: {error}"))
            })?;
            file.sync_all().map_err(|error| {
                SymphonyError::StorageError(format!("failed syncing staged blob: {error}"))
            })?;
            // Hash the staged copy, not the source: a source that changed
            // after `sha256_file` must be caught here.
            let staged_sha = sha256_file(&staged)?;
            if staged_sha != sha256 {
                let _ = fs::remove_file(&staged);
                return Err(SymphonyError::StorageError(format!(
                    "staged blob digest {staged_sha} differs from source digest {sha256}; source mutated while copying"
                )));
            }
            (sha256, bytes_len, staged)
        }
        BlobSource::PathVerified {
            path,
            intended_sha256,
            intended_bytes_len,
        } => {
            let meta = fs::metadata(path).map_err(|error| {
                SymphonyError::StorageError(format!("failed stating {}: {error}", path.display()))
            })?;
            let bytes_len = meta.len();
            if bytes_len != intended_bytes_len {
                return Err(SymphonyError::StorageError(format!(
                    "evidence {} size {} does not match intended size {intended_bytes_len}",
                    path.display(),
                    bytes_len
                )));
            }
            if bytes_len > max_bytes {
                return Err(SymphonyError::StorageError(format!(
                    "evidence is {bytes_len} bytes; max is {max_bytes}"
                )));
            }
            let staged = artifacts_dir.join(unique_temp_name(&intended_sha256));
            fs::copy(path, &staged).map_err(|error| {
                SymphonyError::StorageError(format!(
                    "failed copying evidence into temp {}: {error}",
                    staged.display()
                ))
            })?;
            let file = File::open(&staged).map_err(|error| {
                SymphonyError::StorageError(format!("failed opening staged blob: {error}"))
            })?;
            file.sync_all().map_err(|error| {
                SymphonyError::StorageError(format!("failed syncing staged blob: {error}"))
            })?;
            // The staged copy must hash to the intended digest recorded when
            // the evidence was collected; a post-hash mutation fails closed.
            let staged_sha = sha256_file(&staged)?;
            if staged_sha != intended_sha256 {
                let _ = fs::remove_file(&staged);
                return Err(SymphonyError::StorageError(format!(
                    "evidence {} staged digest {staged_sha} does not match intended digest {intended_sha256}",
                    path.display()
                )));
            }
            (intended_sha256.clone(), intended_bytes_len, staged)
        }
    };

    let dest = blob_path(artifacts_dir, &sha256);
    if dest.exists() {
        let existing = sha256_file(&dest)?;
        let _ = fs::remove_file(&temp_path);
        if existing != sha256 {
            return Err(SymphonyError::StorageError(format!(
                "blob collision at {}: existing digest {existing} != {sha256}",
                dest.display()
            )));
        }
        let existing_len = fs::metadata(&dest)
            .map(|meta| meta.len())
            .unwrap_or(bytes_len);
        if existing_len != bytes_len {
            return Err(SymphonyError::StorageError(format!(
                "blob size mismatch at {}: {existing_len} != {bytes_len}",
                dest.display()
            )));
        }
        return Ok((sha256, bytes_len));
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            SymphonyError::StorageError(format!(
                "failed creating blob parent {}: {error}",
                parent.display()
            ))
        })?;
    }
    fs::rename(&temp_path, &dest).map_err(|error| {
        SymphonyError::StorageError(format!(
            "failed renaming blob to {}: {error}",
            dest.display()
        ))
    })?;
    sync_parent(dest.parent().unwrap_or(artifacts_dir))?;
    Ok((sha256, bytes_len))
}

#[derive(Debug)]
pub enum BlobSource<'a> {
    Bytes(&'a [u8]),
    Path(&'a Path),
    /// Path whose bytes must match the intended content identity recorded when
    /// the evidence was collected. The staged copy is hashed after the source
    /// copy and compared with the intended digest/size before rename.
    PathVerified {
        path: &'a Path,
        intended_sha256: String,
        intended_bytes_len: u64,
    },
}

/// Clone a verified bundle into a fresh workspace directory.
pub fn clone_from_bundle(bundle: &Path, dest_workspace: &Path) -> Result<()> {
    verify_bundle(bundle)?;
    if dest_workspace.exists() {
        return Err(SymphonyError::TriageError(format!(
            "clone destination already exists: {}",
            dest_workspace.display()
        )));
    }
    if let Some(parent) = dest_workspace.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            SymphonyError::StorageError(format!(
                "failed creating clone parent {}: {error}",
                parent.display()
            ))
        })?;
    }
    let output = Command::new("git")
        .args(["clone"])
        .arg(bundle)
        .arg(dest_workspace)
        .output()
        .map_err(|error| {
            SymphonyError::TriageError(format!("git clone from bundle failed: {error}"))
        })?;
    if !output.status.success() {
        return Err(SymphonyError::TriageError(format!(
            "git clone from bundle failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

/// Import a result bundle into a temporary host repository for verification.
///
/// Returns the temporary repo directory (caller owns cleanup via the TempDir).
pub fn import_bundle_to_temp(
    base_bundle: &Path,
    result_bundle: &Path,
) -> Result<tempfile::TempDir> {
    let dir = tempdir().map_err(|error| {
        SymphonyError::StorageError(format!("failed creating temp import dir: {error}"))
    })?;
    let repo = dir.path().join("repo");
    clone_from_bundle(base_bundle, &repo)?;
    let output = Command::new("git")
        .args(["pull"])
        .arg(result_bundle)
        .current_dir(&repo)
        .output()
        .map_err(|error| {
            SymphonyError::TriageError(format!("git pull result bundle failed: {error}"))
        })?;
    if !output.status.success() {
        // Fallback: `git fetch` with an explicit refspec, then checkout that branch.
        let fetch = Command::new("git")
            .args(["fetch"])
            .arg(result_bundle)
            .arg("HEAD:refs/heads/bundle-head")
            .current_dir(&repo)
            .output()
            .map_err(|error| {
                SymphonyError::TriageError(format!("git fetch result bundle failed: {error}"))
            })?;
        if !fetch.status.success() {
            return Err(SymphonyError::TriageError(format!(
                "git import result bundle failed: pull={} fetch={}",
                String::from_utf8_lossy(&output.stderr).trim(),
                String::from_utf8_lossy(&fetch.stderr).trim()
            )));
        }
        run_git(&repo, &["checkout", "bundle-head"])?;
    }
    Ok(dir)
}

/// Import only a result bundle that is self-contained (includes base history).
pub fn import_result_bundle_to_temp(result_bundle: &Path) -> Result<tempfile::TempDir> {
    let dir = tempdir().map_err(|error| {
        SymphonyError::StorageError(format!("failed creating temp import dir: {error}"))
    })?;
    let repo = dir.path().join("repo");
    // Result bundles for base..head may need an empty repo + fetch.
    fs::create_dir_all(&repo).map_err(|error| {
        SymphonyError::StorageError(format!("failed creating import repo: {error}"))
    })?;
    run_git(&repo, &["init"])?;
    let fetch = Command::new("git")
        .args(["fetch"])
        .arg(result_bundle)
        .arg("HEAD:refs/heads/bundle-head")
        .current_dir(&repo)
        .output()
        .map_err(|error| {
            SymphonyError::TriageError(format!("git fetch result bundle failed: {error}"))
        })?;
    if !fetch.status.success() {
        // Try cloning if the bundle is a complete history.
        let _ = fs::remove_dir_all(&repo);
        clone_from_bundle(result_bundle, &repo)?;
        return Ok(dir);
    }
    run_git(&repo, &["checkout", "bundle-head"])?;
    Ok(dir)
}

pub fn cleanup_incomplete_temps(artifacts_dir: &Path) -> Result<u64> {
    if !artifacts_dir.exists() {
        return Ok(0);
    }
    let mut removed = 0u64;
    let entries = fs::read_dir(artifacts_dir).map_err(|error| {
        SymphonyError::StorageError(format!(
            "failed reading artifacts dir {}: {error}",
            artifacts_dir.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            SymphonyError::StorageError(format!("failed reading artifacts entry: {error}"))
        })?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with(TEMP_PREFIX) {
            let _ = fs::remove_file(entry.path());
            removed += 1;
        }
    }
    Ok(removed)
}

fn sync_parent(path: &Path) -> Result<()> {
    let dir = File::open(path).map_err(|error| {
        SymphonyError::StorageError(format!(
            "failed opening parent {} for sync: {error}",
            path.display()
        ))
    })?;
    dir.sync_all().map_err(|error| {
        SymphonyError::StorageError(format!("failed syncing parent {}: {error}", path.display()))
    })?;
    Ok(())
}

fn absolute_output_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|error| {
            SymphonyError::StorageError(format!(
                "failed resolving bundle output path {}: {error}",
                path.display()
            ))
        })
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
    use std::process::Command;
    use tempfile::tempdir;

    fn init_tiny_repo() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        run_git(repo, &["init"]).unwrap();
        run_git(repo, &["config", "user.email", "a3@example.com"]).unwrap();
        run_git(repo, &["config", "user.name", "A3 Test"]).unwrap();
        fs::write(repo.join("README.md"), "hello\n").unwrap();
        run_git(repo, &["add", "README.md"]).unwrap();
        run_git(repo, &["commit", "-m", "init"]).unwrap();
        dir
    }

    fn head_sha(repo: &Path) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn artifacts_dir_derives_from_storage_path() {
        let path = Path::new("/tmp/factory/github.com/acme/repo.sqlite");
        assert_eq!(
            artifacts_dir(path),
            PathBuf::from("/tmp/factory/github.com/acme/repo.sqlite.artifacts")
        );
    }

    #[test]
    fn blob_path_uses_sha256_prefix_layout() {
        let root = Path::new("/tmp/arts");
        let digest = "abcdef0123456789";
        assert_eq!(
            blob_path(root, digest),
            PathBuf::from("/tmp/arts/sha256/ab/abcdef0123456789")
        );
    }

    #[test]
    fn create_verify_clone_base_bundle() {
        let repo = init_tiny_repo();
        let commit = head_sha(repo.path());
        let tmp = tempdir().unwrap();
        let bundle = tmp.path().join("base.bundle");
        create_base_bundle(repo.path(), &commit, &bundle).unwrap();
        verify_bundle(&bundle).unwrap();

        let workspace = tmp.path().join("workspace");
        clone_from_bundle(&bundle, &workspace).unwrap();
        assert_eq!(head_sha(&workspace), commit);
        assert!(workspace.join("README.md").is_file());
    }

    #[test]
    fn relative_bundle_destinations_work_from_a_temporary_git_worktree() {
        let repo = init_tiny_repo();
        let base = head_sha(repo.path());
        fs::write(repo.path().join("code.rs"), "fn main() {}\n").unwrap();
        run_git(repo.path(), &["add", "code.rs"]).unwrap();
        run_git(repo.path(), &["commit", "-m", "impl"]).unwrap();
        let head = head_sha(repo.path());

        let cwd = std::env::current_dir().unwrap();
        let tmp = tempfile::tempdir_in(&cwd).unwrap();
        let relative_dir = tmp.path().strip_prefix(&cwd).unwrap();
        let base_bundle = relative_dir.join("base.bundle");
        let result_bundle = relative_dir.join("result.bundle");

        create_base_bundle(repo.path(), &base, &base_bundle).unwrap();
        create_result_bundle(repo.path(), &base, &head, &result_bundle).unwrap();

        assert!(tmp.path().join("base.bundle").is_file());
        assert!(tmp.path().join("result.bundle").is_file());
        verify_bundle(&base_bundle).unwrap();
        verify_bundle_in(repo.path(), &tmp.path().join("result.bundle")).unwrap();
    }

    #[test]
    fn result_bundle_verifies_standalone_without_a_prerequisite_repo() {
        // #617: the A3 publication path re-verifies the stored result bundle
        // standalone; a thin base..HEAD bundle fails that check.
        let repo = init_tiny_repo();
        let base = head_sha(repo.path());
        fs::write(repo.path().join("code.rs"), "fn main() {}\n").unwrap();
        run_git(repo.path(), &["add", "code.rs"]).unwrap();
        run_git(repo.path(), &["commit", "-m", "impl"]).unwrap();
        let head = head_sha(repo.path());

        let tmp = tempdir().unwrap();
        let result = tmp.path().join("result.bundle");
        create_result_bundle(repo.path(), &base, &head, &result).unwrap();

        // Standalone verification must pass: no repository context exists.
        let empty = tempdir().unwrap();
        run_git(empty.path(), &["init"]).unwrap();
        let output = Command::new("git")
            .args(["bundle", "verify"])
            .arg(&result)
            .current_dir(empty.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "self-contained result bundle must verify standalone: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn result_bundle_and_atomic_store() {
        let repo = init_tiny_repo();
        let base = head_sha(repo.path());
        fs::write(repo.path().join("code.rs"), "fn main() {}\n").unwrap();
        run_git(repo.path(), &["add", "code.rs"]).unwrap();
        run_git(repo.path(), &["commit", "-m", "impl"]).unwrap();
        let head = head_sha(repo.path());

        let tmp = tempdir().unwrap();
        let result = tmp.path().join("result.bundle");
        create_result_bundle(repo.path(), &base, &head, &result).unwrap();

        let arts = tmp.path().join("db.artifacts");
        let (digest, len) =
            store_blob_atomic(&arts, BlobSource::Path(&result), 10 * 1024 * 1024).unwrap();
        assert_eq!(digest, sha256_file(&result).unwrap());
        assert!(len > 0);
        assert!(blob_path(&arts, &digest).is_file());

        // Re-store is idempotent and re-verifies.
        let (digest2, len2) =
            store_blob_atomic(&arts, BlobSource::Path(&result), 10 * 1024 * 1024).unwrap();
        assert_eq!(digest, digest2);
        assert_eq!(len, len2);
    }

    #[test]
    fn cleanup_removes_incomplete_temps() {
        let tmp = tempdir().unwrap();
        let arts = tmp.path().join("arts");
        fs::create_dir_all(&arts).unwrap();
        fs::write(arts.join(format!("{TEMP_PREFIX}orphan")), b"x").unwrap();
        fs::write(arts.join("keep-me"), b"y").unwrap();
        assert_eq!(cleanup_incomplete_temps(&arts).unwrap(), 1);
        assert!(!arts.join(format!("{TEMP_PREFIX}orphan")).exists());
        assert!(arts.join("keep-me").exists());
    }

    #[test]
    fn store_blob_rejects_oversize() {
        let tmp = tempdir().unwrap();
        let arts = tmp.path().join("arts");
        let err = store_blob_atomic(&arts, BlobSource::Bytes(&[0u8; 64]), 16).unwrap_err();
        assert!(err.to_string().contains("max_bundle_bytes"));
    }

    #[test]
    fn verified_blob_rejects_a_mutated_source_beneath_a_wrong_digest() {
        let tmp = tempdir().unwrap();
        let arts = tmp.path().join("arts");
        let source = tmp.path().join("evidence.txt");
        fs::write(&source, b"immutable bytes").unwrap();
        let intended = sha256_file(&source).unwrap();
        let len = fs::metadata(&source).unwrap().len();

        // Adversarial mutation after collection: the file bytes change but the
        // recorded digest still describes the old content.
        fs::write(&source, b"tampered bytes!").unwrap();
        let err = store_blob_atomic(
            &arts,
            BlobSource::PathVerified {
                path: &source,
                intended_sha256: intended.clone(),
                intended_bytes_len: len,
            },
            1024,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("does not match intended digest"),
            "mutated evidence must fail closed: {err}"
        );
        assert!(
            !blob_path(&arts, &intended).exists(),
            "no blob may be stored beneath the stale digest"
        );

        // Size mismatch also fails closed.
        fs::write(&source, b"short").unwrap();
        let err = store_blob_atomic(
            &arts,
            BlobSource::PathVerified {
                path: &source,
                intended_sha256: intended.clone(),
                intended_bytes_len: len,
            },
            1024,
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not match intended size"));
    }

    #[test]
    fn verified_blob_stores_when_staged_copy_matches_identity() {
        let tmp = tempdir().unwrap();
        let arts = tmp.path().join("arts");
        let source = tmp.path().join("evidence.txt");
        fs::write(&source, b"stable bytes").unwrap();
        let intended = sha256_file(&source).unwrap();
        let len = fs::metadata(&source).unwrap().len();

        let (digest, stored_len) = store_blob_atomic(
            &arts,
            BlobSource::PathVerified {
                path: &source,
                intended_sha256: intended.clone(),
                intended_bytes_len: len,
            },
            1024,
        )
        .unwrap();
        assert_eq!(digest, intended);
        assert_eq!(stored_len, len);
        assert!(blob_path(&arts, &digest).is_file());
        assert_eq!(
            fs::read(blob_path(&arts, &digest)).unwrap(),
            b"stable bytes"
        );
        // No incomplete temp remains behind.
        assert_eq!(cleanup_incomplete_temps(&arts).unwrap(), 0);
    }

    #[test]
    fn plain_path_source_still_hashes_the_staged_copy() {
        let tmp = tempdir().unwrap();
        let arts = tmp.path().join("arts");
        let source = tmp.path().join("plain.txt");
        fs::write(&source, b"plain content").unwrap();

        let (digest, len) = store_blob_atomic(&arts, BlobSource::Path(&source), 1024).unwrap();
        assert_eq!(digest, sha256_file(&source).unwrap());
        assert_eq!(len, 13);
        assert!(blob_path(&arts, &digest).is_file());
    }
}
