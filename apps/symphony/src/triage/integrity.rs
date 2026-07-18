use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoBaseline {
    pub head: String,
    pub submodules: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityError {
    GitCommandFailed {
        command: String,
        stderr: String,
    },
    HeadChanged {
        expected: String,
        actual: String,
    },
    SubmoduleChanged {
        path: String,
        expected: String,
        actual: String,
    },
    SourceTreeDirty {
        entries: Vec<String>,
    },
}

impl fmt::Display for IntegrityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GitCommandFailed { command, stderr } => {
                write!(f, "git command failed: {command}: {stderr}")
            }
            Self::HeadChanged { expected, actual } => {
                write!(f, "repository HEAD changed from {expected} to {actual}")
            }
            Self::SubmoduleChanged {
                path,
                expected,
                actual,
            } => write!(f, "submodule {path} changed from {expected} to {actual}"),
            Self::SourceTreeDirty { entries } => {
                write!(f, "source tree has dirty entries: {}", entries.join(", "))
            }
        }
    }
}

impl std::error::Error for IntegrityError {}

pub type IntegrityResult<T> = std::result::Result<T, IntegrityError>;

pub fn capture_baseline(workspace: &Path) -> IntegrityResult<RepoBaseline> {
    Ok(RepoBaseline {
        head: git_stdout(workspace, &["rev-parse", "HEAD"])?,
        submodules: capture_submodules(workspace)?,
    })
}

pub fn check_integrity(workspace: &Path, baseline: &RepoBaseline) -> IntegrityResult<()> {
    let current_head = git_stdout(workspace, &["rev-parse", "HEAD"])?;
    if current_head != baseline.head {
        return Err(IntegrityError::HeadChanged {
            expected: baseline.head.clone(),
            actual: current_head,
        });
    }

    let current_submodules = capture_submodules(workspace)?;
    for (path, expected) in &baseline.submodules {
        let actual = current_submodules.get(path).cloned().unwrap_or_default();
        if actual != *expected {
            return Err(IntegrityError::SubmoduleChanged {
                path: path.clone(),
                expected: expected.clone(),
                actual,
            });
        }
    }

    let dirty_entries = dirty_source_entries(workspace)?;
    if !dirty_entries.is_empty() {
        return Err(IntegrityError::SourceTreeDirty {
            entries: dirty_entries,
        });
    }

    Ok(())
}

/// Alias used by the triage runner for repository integrity checks.
pub fn check_repository_integrity(
    workspace: &Path,
    baseline: &RepoBaseline,
) -> IntegrityResult<()> {
    check_integrity(workspace, baseline)
}

fn capture_submodules(workspace: &Path) -> IntegrityResult<BTreeMap<String, String>> {
    let output = git_stdout(workspace, &["submodule", "status", "--recursive"])?;
    let mut submodules = BTreeMap::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let commit = trimmed
            .trim_start_matches(['-', '+', 'U', ' '])
            .split_whitespace()
            .next()
            .unwrap_or_default();
        let path = trimmed.split_whitespace().nth(1).unwrap_or_default();
        if !commit.is_empty() && !path.is_empty() {
            submodules.insert(path.to_string(), commit.to_string());
        }
    }
    Ok(submodules)
}

fn dirty_source_entries(workspace: &Path) -> IntegrityResult<Vec<String>> {
    let output = git_stdout(
        workspace,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    let mut dirty = Vec::new();
    for line in output.lines() {
        if line.len() < 4 {
            continue;
        }
        let status = &line[..2];
        let path = line[3..].to_string();
        if status == "??" {
            if is_source_path(&PathBuf::from(&path)) {
                dirty.push(line.to_string());
            }
        } else {
            dirty.push(line.to_string());
        }
    }
    Ok(dirty)
}

fn is_source_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return true;
    };

    matches!(
        extension.to_ascii_lowercase().as_str(),
        "c" | "cc"
            | "cpp"
            | "cxx"
            | "css"
            | "go"
            | "h"
            | "hpp"
            | "html"
            | "java"
            | "js"
            | "jsx"
            | "json"
            | "md"
            | "py"
            | "rs"
            | "sh"
            | "sql"
            | "toml"
            | "ts"
            | "tsx"
            | "yaml"
            | "yml"
    )
}

fn git_stdout(workspace: &Path, args: &[&str]) -> IntegrityResult<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .map_err(|err| IntegrityError::GitCommandFailed {
            command: format!("git {}", args.join(" ")),
            stderr: err.to_string(),
        })?;

    if !output.status.success() {
        return Err(IntegrityError::GitCommandFailed {
            command: format!("git {}", args.join(" ")),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn run(path: &Path, args: &[&str]) {
        let output = Command::new(args[0])
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

    fn repo() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        run(temp.path(), &["git", "init"]);
        run(
            temp.path(),
            &["git", "config", "user.email", "test@example.com"],
        );
        run(temp.path(), &["git", "config", "user.name", "Test"]);
        fs::write(temp.path().join("src.rs"), "fn main() {}\n").unwrap();
        fs::write(temp.path().join(".gitignore"), "target/\n").unwrap();
        run(temp.path(), &["git", "add", "."]);
        run(temp.path(), &["git", "commit", "-m", "initial"]);
        temp
    }

    #[test]
    fn clean_repo_passes_integrity_check() {
        let repo = repo();
        let baseline = capture_baseline(repo.path()).unwrap();

        check_integrity(repo.path(), &baseline).unwrap();
    }

    #[test]
    fn changed_head_fails_integrity_check() {
        let repo = repo();
        let baseline = capture_baseline(repo.path()).unwrap();
        fs::write(repo.path().join("new.rs"), "fn new() {}\n").unwrap();
        run(repo.path(), &["git", "add", "."]);
        run(repo.path(), &["git", "commit", "-m", "change"]);

        assert!(matches!(
            check_integrity(repo.path(), &baseline),
            Err(IntegrityError::HeadChanged { .. })
        ));
    }

    #[test]
    fn staged_unstaged_and_untracked_source_fail() {
        let repo = repo();
        let baseline = capture_baseline(repo.path()).unwrap();
        fs::write(
            repo.path().join("src.rs"),
            "fn main() { println!(\"x\"); }\n",
        )
        .unwrap();
        assert!(matches!(
            check_integrity(repo.path(), &baseline),
            Err(IntegrityError::SourceTreeDirty { .. })
        ));

        run(repo.path(), &["git", "checkout", "--", "src.rs"]);
        fs::write(repo.path().join("new.rs"), "fn new() {}\n").unwrap();
        assert!(matches!(
            check_integrity(repo.path(), &baseline),
            Err(IntegrityError::SourceTreeDirty { .. })
        ));

        run(repo.path(), &["git", "add", "new.rs"]);
        assert!(matches!(
            check_integrity(repo.path(), &baseline),
            Err(IntegrityError::SourceTreeDirty { .. })
        ));
    }

    #[test]
    fn ignored_build_output_is_allowed() {
        let repo = repo();
        let baseline = capture_baseline(repo.path()).unwrap();
        fs::create_dir(repo.path().join("target")).unwrap();
        fs::write(repo.path().join("target/output.rs"), "generated").unwrap();

        check_integrity(repo.path(), &baseline).unwrap();
    }
}
