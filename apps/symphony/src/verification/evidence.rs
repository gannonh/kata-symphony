//! Bounded evidence collection from the attempt-owned evidence directory.
//!
//! Only regular files beneath `$SYMPHONY_EVIDENCE_DIR` are accepted.
//! Traversal errors, symlinks, special files, file-count overflow,
//! aggregate-size overflow, and post-hash mutation all fail closed. Bytes are
//! stored in the content-addressed artifact store under the recorded digest;
//! only metadata ever leaves the store.

use std::fs;
use std::path::Path;

use chrono::Utc;

use crate::error::{Result, SymphonyError};
use crate::implementation::bundle::{sha256_file, store_blob_atomic, BlobSource};
use crate::verification::domain::{
    VerificationEvidenceRecord, VERIFICATION_EVIDENCE_PATH_MAX_BYTES,
};

/// Collect and durably store every regular file under `evidence_dir`.
pub fn collect_evidence(
    evidence_dir: &Path,
    artifacts_dir: &Path,
    run_id: &str,
    attempt_id: &str,
    max_files: usize,
    max_bytes: u64,
) -> Result<Vec<VerificationEvidenceRecord>> {
    if !evidence_dir.is_dir() {
        return Err(SymphonyError::TriageError(format!(
            "evidence directory {} is not a directory",
            evidence_dir.display()
        )));
    }
    let mut records = Vec::new();
    let mut aggregate_bytes: u64 = 0;
    walk_evidence_dir(
        evidence_dir,
        evidence_dir,
        artifacts_dir,
        run_id,
        attempt_id,
        max_files,
        max_bytes,
        &mut aggregate_bytes,
        &mut records,
    )?;
    Ok(records)
}

fn walk_evidence_dir(
    root: &Path,
    dir: &Path,
    artifacts_dir: &Path,
    run_id: &str,
    attempt_id: &str,
    max_files: usize,
    max_bytes: u64,
    aggregate_bytes: &mut u64,
    records: &mut Vec<VerificationEvidenceRecord>,
) -> Result<()> {
    let entries = fs::read_dir(dir).map_err(|error| {
        SymphonyError::TriageError(format!(
            "failed reading evidence directory {}: {error}",
            dir.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            SymphonyError::TriageError(format!(
                "failed reading evidence entry in {}: {error}",
                dir.display()
            ))
        })?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|error| {
            SymphonyError::TriageError(format!(
                "failed stating evidence path {}: {error}",
                path.display()
            ))
        })?;
        if meta.file_type().is_symlink() {
            return Err(SymphonyError::TriageError(format!(
                "evidence symlink rejected: {}",
                path.display()
            )));
        }
        if meta.is_dir() {
            walk_evidence_dir(
                root,
                &path,
                artifacts_dir,
                run_id,
                attempt_id,
                max_files,
                max_bytes,
                aggregate_bytes,
                records,
            )?;
            continue;
        }
        if !meta.is_file() {
            return Err(SymphonyError::TriageError(format!(
                "evidence special file rejected: {}",
                path.display()
            )));
        }
        if records.len() >= max_files {
            return Err(SymphonyError::TriageError(format!(
                "evidence file count exceeds max_evidence_files={max_files}"
            )));
        }
        let relative_path = path.strip_prefix(root).map_err(|error| {
            SymphonyError::TriageError(format!("evidence path escapes root: {error}"))
        })?;
        let relative_path = relative_path.to_string_lossy().to_string();
        if relative_path.len() > VERIFICATION_EVIDENCE_PATH_MAX_BYTES {
            return Err(SymphonyError::TriageError(format!(
                "evidence path exceeds {VERIFICATION_EVIDENCE_PATH_MAX_BYTES} bytes: {relative_path}"
            )));
        }
        *aggregate_bytes = aggregate_bytes.saturating_add(meta.len());
        if *aggregate_bytes > max_bytes {
            return Err(SymphonyError::TriageError(format!(
                "evidence aggregate size exceeds max_evidence_bytes={max_bytes}"
            )));
        }
        // The intended identity is recorded before storage; the atomic blob
        // helper re-hashes the staged copy and fails if the file mutated.
        let intended_sha256 = sha256_file(&path)?;
        let intended_bytes_len = meta.len();
        let (sha256, bytes_len) = store_blob_atomic(
            artifacts_dir,
            BlobSource::PathVerified {
                path: &path,
                intended_sha256: intended_sha256.clone(),
                intended_bytes_len,
            },
            max_bytes,
        )?;
        debug_assert_eq!(sha256, intended_sha256);
        records.push(VerificationEvidenceRecord {
            evidence_id: uuid::Uuid::new_v4().to_string(),
            run_id: run_id.to_string(),
            attempt_id: attempt_id.to_string(),
            relative_path,
            sha256,
            bytes_len,
            collected_at: Utc::now(),
        });
    }
    Ok(())
}

/// Remove an attempt-owned evidence tree, refusing paths that are not
/// `evidence` directories inside a `verification-<attempt_id>` root.
pub fn cleanup_evidence_dir(evidence_dir: &Path, workspace_root: &Path) -> Result<()> {
    let Some(name) = evidence_dir.file_name().and_then(|name| name.to_str()) else {
        return Err(SymphonyError::TriageError(
            "evidence path has no usable name".to_string(),
        ));
    };
    if name != "evidence" {
        return Err(SymphonyError::TriageError(format!(
            "refusing to remove non-evidence path {}",
            evidence_dir.display()
        )));
    }
    let Some(parent) = evidence_dir.parent() else {
        return Err(SymphonyError::TriageError(
            "evidence path has no parent".to_string(),
        ));
    };
    let Some(parent_name) = parent.file_name().and_then(|name| name.to_str()) else {
        return Err(SymphonyError::TriageError(
            "evidence parent has no usable name".to_string(),
        ));
    };
    if !parent_name.starts_with("verification-") {
        return Err(SymphonyError::TriageError(format!(
            "refusing to remove evidence outside a verification attempt root: {}",
            evidence_dir.display()
        )));
    }
    let Some(grandparent) = parent.parent() else {
        return Err(SymphonyError::TriageError(
            "evidence parent has no grandparent".to_string(),
        ));
    };
    let workspace_root = workspace_root.canonicalize().unwrap_or_else(|_| {
        workspace_root.to_path_buf()
    });
    let grandparent = grandparent.canonicalize().unwrap_or_else(|_| grandparent.to_path_buf());
    if grandparent != workspace_root {
        return Err(SymphonyError::TriageError(format!(
            "refusing to remove evidence outside the configured workspace root: {}",
            evidence_dir.display()
        )));
    }
    if evidence_dir.exists() {
        fs::remove_dir_all(evidence_dir).map_err(|error| {
            SymphonyError::TriageError(format!(
                "failed removing evidence dir {}: {error}",
                evidence_dir.display()
            ))
        })?;
    }
    Ok(())
}

/// Deterministic test layout helper: build a small evidence tree.
#[cfg(test)]
pub(crate) fn write_fixture(evidence_dir: &Path, entries: &[(&str, &[u8])]) {
    for (relative, bytes) in entries {
        let path = evidence_dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn collects_bounded_regular_files_by_digest() {
        let dir = tempdir().unwrap();
        let evidence = dir.path().join("evidence");
        fs::create_dir_all(&evidence).unwrap();
        write_fixture(
            &evidence,
            &[
                ("reports/summary.json", b"{\"ok\":true}"),
                ("logs/run.log", b"all good\n"),
            ],
        );
        let artifacts = dir.path().join("db.artifacts");
        let records = collect_evidence(
            &evidence,
            &artifacts,
            "run-1",
            "attempt-1",
            10,
            1024 * 1024,
        )
        .unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].relative_path, "logs/run.log");
        assert_eq!(records[0].run_id, "run-1");
        assert_eq!(records[0].attempt_id, "attempt-1");
        // Bytes landed in the content-addressed store under the digest.
        let blob = artifacts
            .join("sha256")
            .join(&records[0].sha256[..2])
            .join(&records[0].sha256);
        assert_eq!(fs::read(&blob).unwrap(), b"all good\n");
        assert_eq!(records[1].relative_path, "reports/summary.json");
    }

    #[test]
    fn rejects_symlinks() {
        #[cfg(unix)]
        {
            let dir = tempdir().unwrap();
            let evidence = dir.path().join("evidence");
            fs::create_dir_all(&evidence).unwrap();
            write_fixture(&evidence, &[("real.txt", b"x")]);
            std::os::unix::fs::symlink(
                dir.path().join("real.txt"),
                evidence.join("link.txt"),
            )
            .unwrap();
            let err = collect_evidence(
                &evidence,
                &dir.path().join("arts"),
                "run",
                "att",
                10,
                1024,
            )
            .unwrap_err();
            assert!(err.to_string().contains("symlink"));
        }
    }

    #[test]
    fn rejects_file_count_overflow() {
        let dir = tempdir().unwrap();
        let evidence = dir.path().join("evidence");
        fs::create_dir_all(&evidence).unwrap();
        write_fixture(&evidence, &[("a.txt", b"a"), ("b.txt", b"b")]);
        let err = collect_evidence(
            &evidence,
            &dir.path().join("arts"),
            "run",
            "att",
            1,
            1024,
        )
        .unwrap_err();
        assert!(err.to_string().contains("max_evidence_files"));
    }

    #[test]
    fn rejects_aggregate_size_overflow() {
        let dir = tempdir().unwrap();
        let evidence = dir.path().join("evidence");
        fs::create_dir_all(&evidence).unwrap();
        write_fixture(&evidence, &[("a.txt", b"aaaa")]);
        let err = collect_evidence(
            &evidence,
            &dir.path().join("arts"),
            "run",
            "att",
            10,
            2,
        )
        .unwrap_err();
        assert!(err.to_string().contains("max_evidence_bytes"));
    }

    #[test]
    fn rejects_special_files() {
        #[cfg(unix)]
        {
            let dir = tempdir().unwrap();
            let evidence = dir.path().join("evidence");
            fs::create_dir_all(&evidence).unwrap();
            std::process::Command::new("mkfifo")
                .arg(evidence.join("fifo"))
                .status()
                .unwrap();
            let err = collect_evidence(
                &evidence,
                &dir.path().join("arts"),
                "run",
                "att",
                10,
                1024,
            )
            .unwrap_err();
            assert!(err.to_string().contains("special file"));
        }
    }

    #[test]
    fn post_hash_mutation_fails_closed() {
        let dir = tempdir().unwrap();
        let evidence = dir.path().join("evidence");
        fs::create_dir_all(&evidence).unwrap();
        let file = evidence.join("volatile.txt");
        fs::write(&file, b"original").unwrap();
        let intended = sha256_file(&file).unwrap();
        // Mutate after the intended digest was recorded.
        fs::write(&file, b"mutated!").unwrap();
        let err = store_blob_atomic(
            &dir.path().join("arts"),
            BlobSource::PathVerified {
                path: &file,
                intended_sha256: intended,
                intended_bytes_len: 8,
            },
            1024,
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not match intended"));
    }

    #[test]
    fn cleanup_accepts_only_attempt_owned_evidence_dirs() {
        let root = tempdir().unwrap();
        let evidence = root.path().join("verification-att-1/evidence");
        fs::create_dir_all(&evidence).unwrap();
        fs::write(evidence.join("x.txt"), b"x").unwrap();
        cleanup_evidence_dir(&evidence, root.path()).unwrap();
        assert!(!evidence.exists());

        // A verification-* dir elsewhere must be refused.
        let outside = root.path().join("elsewhere");
        let other = outside.join("verification-att-2/evidence");
        fs::create_dir_all(&other).unwrap();
        let err = cleanup_evidence_dir(&other, root.path()).unwrap_err();
        assert!(err.to_string().contains("outside the configured workspace root"));
        assert!(other.exists());
    }

    #[test]
    fn rejects_non_evidence_names() {
        let root = tempdir().unwrap();
        let path = root.path().join("verification-att-1/workspace");
        fs::create_dir_all(&path).unwrap();
        let err = cleanup_evidence_dir(&path, root.path()).unwrap_err();
        assert!(err.to_string().contains("non-evidence"));
    }

    #[test]
    fn cleanup_requires_named_parent_under_workspace_root() {
        let root = tempdir().unwrap();
        let path = root.path().join("evidence");
        fs::create_dir_all(&path).unwrap();
        let err = cleanup_evidence_dir(&path, root.path()).unwrap_err();
        assert!(err.to_string().contains("verification attempt root"));
    }
}
