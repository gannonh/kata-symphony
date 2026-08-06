use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::domain::TrackerConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubTokenSource {
    TrackerApiKey,
    GhTokenEnv,
    GithubTokenEnv,
    GhCli,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGithubToken {
    pub token: String,
    pub source: GithubTokenSource,
}

const GITHUB_TOKEN_MISSING_MESSAGE: &str =
    "GitHub token required when tracker.kind is github. Set tracker.api_key, GH_TOKEN/GITHUB_TOKEN, or authenticate gh CLI via `gh auth login` (local fallback).";
const GH_CLI_FALLBACK_DISABLE_VALUES: [&str; 5] = ["0", "false", "no", "off", "disabled"];
const GH_CLI_FALLBACK_TIMEOUT: Duration = Duration::from_secs(2);

pub fn github_token_missing_message() -> &'static str {
    GITHUB_TOKEN_MISSING_MESSAGE
}

pub fn resolve_github_token(tracker: &TrackerConfig) -> Option<ResolvedGithubToken> {
    tracker
        .api_key
        .as_ref()
        .map(|api_key| api_key.as_str().trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|token| ResolvedGithubToken {
            token,
            source: GithubTokenSource::TrackerApiKey,
        })
        .or_else(|| {
            crate::credential_env::value("GH_TOKEN")
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .map(|token| ResolvedGithubToken {
                    token,
                    source: GithubTokenSource::GhTokenEnv,
                })
        })
        .or_else(|| {
            crate::credential_env::value("GITHUB_TOKEN")
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .map(|token| ResolvedGithubToken {
                    token,
                    source: GithubTokenSource::GithubTokenEnv,
                })
        })
        .or_else(|| {
            resolve_github_token_from_gh_cli(&tracker.endpoint).map(|token| ResolvedGithubToken {
                token,
                source: GithubTokenSource::GhCli,
            })
        })
}

pub fn github_token_source_name(source: GithubTokenSource) -> &'static str {
    match source {
        GithubTokenSource::TrackerApiKey => "tracker.api_key",
        GithubTokenSource::GhTokenEnv => "GH_TOKEN",
        GithubTokenSource::GithubTokenEnv => "GITHUB_TOKEN",
        GithubTokenSource::GhCli => "gh auth token",
    }
}

fn is_gh_cli_fallback_enabled() -> bool {
    let raw = std::env::var("SYMPHONY_GITHUB_ENABLE_GH_CLI_FALLBACK")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    !GH_CLI_FALLBACK_DISABLE_VALUES.contains(&raw.as_str())
}

fn resolve_github_hostname_for_gh_cli(endpoint: &str) -> String {
    let trimmed = endpoint.trim();
    if trimmed.is_empty() {
        return "github.com".to_string();
    }

    reqwest::Url::parse(trimmed)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(str::trim)
                .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
                .filter(|host| !host.is_empty())
                .map(|host| {
                    if host == "api.github.com" {
                        "github.com".to_string()
                    } else {
                        host
                    }
                })
        })
        .unwrap_or_else(|| "github.com".to_string())
}

fn resolve_github_token_from_gh_cli(endpoint: &str) -> Option<String> {
    if !is_gh_cli_fallback_enabled() {
        return None;
    }

    let hostname = resolve_github_hostname_for_gh_cli(endpoint);
    let mut child = Command::new("gh")
        .args(["auth", "token", "--hostname", hostname.as_str()])
        .env("GH_PROMPT_DISABLED", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }

                let mut stdout = Vec::new();
                if let Some(mut pipe) = child.stdout.take() {
                    if pipe.read_to_end(&mut stdout).is_err() {
                        return None;
                    }
                }

                let token = String::from_utf8(stdout).ok()?.trim().to_string();
                if token.is_empty() {
                    return None;
                }

                return Some(token);
            }
            Ok(None) => {
                if started.elapsed() >= GH_CLI_FALLBACK_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_gh_cli_fallback_enabled, resolve_github_hostname_for_gh_cli, resolve_github_token,
        GithubTokenSource,
    };
    use crate::domain::{ApiKey, TrackerConfig};
    use serial_test::serial;

    fn with_env<T>(values: &[(&str, Option<&str>)], f: impl FnOnce() -> T) -> T {
        let mut previous: Vec<(String, Option<String>)> = Vec::with_capacity(values.len());
        for (key, value) in values {
            previous.push(((*key).to_string(), std::env::var(key).ok()));
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }

        let result = f();

        for (key, value) in previous {
            match value {
                Some(value) => std::env::set_var(&key, value),
                None => std::env::remove_var(&key),
            }
        }

        result
    }

    #[test]
    #[serial]
    fn resolve_github_token_prefers_tracker_api_key() {
        with_env(
            &[
                ("SYMPHONY_GITHUB_ENABLE_GH_CLI_FALLBACK", Some("0")),
                ("GH_TOKEN", Some("gh-env")),
                ("GITHUB_TOKEN", Some("github-env")),
            ],
            || {
                let mut tracker = TrackerConfig::default();
                tracker.api_key = Some(ApiKey::new("config-token"));
                let resolved = resolve_github_token(&tracker).expect("token should resolve");
                assert_eq!(resolved.token, "config-token");
                assert_eq!(resolved.source, GithubTokenSource::TrackerApiKey);
            },
        );
    }

    #[test]
    #[serial]
    fn resolve_github_token_prefers_gh_token_over_github_token() {
        with_env(
            &[
                ("SYMPHONY_GITHUB_ENABLE_GH_CLI_FALLBACK", Some("0")),
                ("GH_TOKEN", Some("gh-env")),
                ("GITHUB_TOKEN", Some("github-env")),
            ],
            || {
                let tracker = TrackerConfig::default();
                let resolved = resolve_github_token(&tracker).expect("token should resolve");
                assert_eq!(resolved.token, "gh-env");
                assert_eq!(resolved.source, GithubTokenSource::GhTokenEnv);
            },
        );
    }

    #[test]
    #[serial]
    fn resolve_github_token_returns_none_when_no_sources_available() {
        with_env(
            &[
                ("SYMPHONY_GITHUB_ENABLE_GH_CLI_FALLBACK", Some("0")),
                ("GH_TOKEN", None),
                ("GITHUB_TOKEN", None),
            ],
            || {
                let tracker = TrackerConfig::default();
                let resolved = resolve_github_token(&tracker);
                assert!(resolved.is_none());
            },
        );
    }

    #[test]
    #[serial]
    fn gh_cli_fallback_disable_values_match_cli_behavior() {
        with_env(
            &[("SYMPHONY_GITHUB_ENABLE_GH_CLI_FALLBACK", Some("false"))],
            || {
                assert!(!is_gh_cli_fallback_enabled());
            },
        );

        with_env(
            &[("SYMPHONY_GITHUB_ENABLE_GH_CLI_FALLBACK", Some("disabled"))],
            || {
                assert!(!is_gh_cli_fallback_enabled());
            },
        );

        with_env(
            &[("SYMPHONY_GITHUB_ENABLE_GH_CLI_FALLBACK", Some(""))],
            || {
                assert!(is_gh_cli_fallback_enabled());
            },
        );
    }

    #[test]
    fn resolve_github_hostname_for_gh_cli_prefers_endpoint_host() {
        assert_eq!(
            resolve_github_hostname_for_gh_cli("https://ghe.example.com/api/v3/graphql"),
            "ghe.example.com"
        );
        assert_eq!(
            resolve_github_hostname_for_gh_cli("https://api.github.com"),
            "github.com"
        );
        assert_eq!(
            resolve_github_hostname_for_gh_cli("https://api.github.com./graphql"),
            "github.com"
        );
        assert_eq!(
            resolve_github_hostname_for_gh_cli("not-a-url"),
            "github.com"
        );
    }
}
