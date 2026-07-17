use crate::triage::domain::StorageConfig;
use std::path::{Path, PathBuf};

pub const DEFAULT_DB_FILE_NAME: &str = "triage.sqlite3";

pub fn resolve_storage_path(
    config: &StorageConfig,
    forge_host: &str,
    owner: &str,
    repo: &str,
) -> PathBuf {
    if let Some(path) = config
        .path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return expand_path(path);
    }

    platform_data_dir()
        .join("symphony")
        .join("triage")
        .join(namespace_segment(forge_host))
        .join(namespace_segment(owner))
        .join(namespace_segment(repo))
        .join(DEFAULT_DB_FILE_NAME)
}

pub fn storage_path_for_log(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

pub fn lock_path_for_storage(path: &Path) -> PathBuf {
    let mut lock_path = path.to_path_buf();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(DEFAULT_DB_FILE_NAME);
    lock_path.set_file_name(format!("{file_name}.lock"));
    lock_path
}

fn expand_path(path: &str) -> PathBuf {
    let expanded_env = expand_env(path);
    if expanded_env == "~" {
        return home_dir();
    }
    if let Some(rest) = expanded_env.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(expanded_env)
}

fn expand_env(value: &str) -> String {
    if let Some(name) = value.strip_prefix('$') {
        if !name.is_empty()
            && name
                .chars()
                .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        {
            return std::env::var(name).unwrap_or_default();
        }
    }
    value.to_string()
}

fn platform_data_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join("AppData").join("Roaming"))
    } else if cfg!(target_os = "macos") {
        home_dir().join("Library").join("Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".local").join("share"))
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn namespace_segment(value: &str) -> String {
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if normalized.is_empty() {
        "unknown".to_string()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_path_wins() {
        let config = StorageConfig {
            path: Some("/tmp/symphony.db".to_string()),
            busy_timeout_ms: 5_000,
        };

        assert_eq!(
            resolve_storage_path(&config, "github.com", "Owner", "Repo"),
            PathBuf::from("/tmp/symphony.db")
        );
    }

    #[test]
    fn default_path_is_namespaced_and_sanitized() {
        let path = resolve_storage_path(
            &StorageConfig::default(),
            "GitHub.COM",
            "Kata Org",
            "Repo/Name",
        );
        let rendered = path.to_string_lossy();

        assert!(rendered.contains("github.com"));
        assert!(rendered.contains("kata-org"));
        assert!(rendered.contains("repo-name"));
        assert!(rendered.ends_with(DEFAULT_DB_FILE_NAME));
    }

    #[test]
    fn lock_path_is_adjacent_to_database() {
        assert_eq!(
            lock_path_for_storage(Path::new("/tmp/state.db")),
            PathBuf::from("/tmp/state.db.lock")
        );
    }
}
