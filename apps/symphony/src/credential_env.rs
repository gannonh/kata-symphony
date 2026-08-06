//! Credential environment capture and scrubbing.
//!
//! Symphony consumes credential-bearing environment variables at startup, then
//! removes them from its own process environment so local verification
//! payloads cannot read them back via `/proc/<parent>/environ` and child
//! processes that inherit the environment do not carry them. Values are
//! captured first and forwarded explicitly to the agent subprocesses that
//! legitimately need them (GitHub-authenticated sessions).

use std::collections::HashMap;
use std::sync::OnceLock;

/// Credential-bearing environment variables Symphony consumes at startup and
/// scrubs from its own process environment once configuration is loaded.
pub const CREDENTIAL_ENV_VARS: [&str; 4] = [
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "KATA_GITHUB_TOKEN",
    "LINEAR_API_KEY",
];

/// GitHub credential vars forwarded to agent session subprocesses.
const GITHUB_CREDENTIAL_VARS: [&str; 3] = ["GH_TOKEN", "GITHUB_TOKEN", "KATA_GITHUB_TOKEN"];

static CAPTURED: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Snapshot credential env vars before they are scrubbed from the process
/// environment. Idempotent: the first call wins.
pub fn capture() {
    let _ = CAPTURED.get_or_init(|| {
        CREDENTIAL_ENV_VARS
            .iter()
            .filter_map(|name| {
                std::env::var(name)
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| (name.to_string(), value))
            })
            .collect()
    });
}

/// Remove credential env vars from the process environment.
///
/// No-op unless [`capture`] ran first, so unit tests that never captured are
/// unaffected.
pub fn scrub() {
    if CAPTURED.get().is_none() {
        return;
    }
    for name in CREDENTIAL_ENV_VARS {
        std::env::remove_var(name);
    }
}

/// Value of a credential env var.
///
/// Prefers the live environment (tests and pre-scrub startup) and falls back
/// to the startup snapshot once the process environment has been scrubbed.
pub fn value(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => CAPTURED.get().and_then(|map| map.get(name)).cloned(),
    }
}

/// The captured GitHub credential vars as (name, value) pairs, for explicit
/// forwarding into agent session subprocesses.
pub fn github_credentials() -> Vec<(String, String)> {
    GITHUB_CREDENTIAL_VARS
        .iter()
        .filter_map(|name| value(name).map(|value| (name.to_string(), value)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // KATA_GITHUB_TOKEN is a scrubbed var no other test mutates, so capture
    // tests cannot race with env-manipulating tests elsewhere in the crate.
    // Within this module, tests are serialized: capture() is once-only and
    // every test mutates the shared process environment.
    const TEST_VAR: &str = "KATA_GITHUB_TOKEN";

    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_test_env(value: &str, body: impl FnOnce()) {
        let original = std::env::var(TEST_VAR).ok();
        std::env::set_var(TEST_VAR, value);
        body();
        match original {
            Some(original) => std::env::set_var(TEST_VAR, original),
            None => std::env::remove_var(TEST_VAR),
        }
    }

    #[test]
    fn capture_and_scrub_roundtrip() {
        let _guard = TEST_LOCK.lock().unwrap();
        with_test_env("snapshot-secret", || {
            capture();
            scrub();
            // The live environment no longer carries the value...
            assert_eq!(std::env::var(TEST_VAR), Err(std::env::VarError::NotPresent));
            // ...but resolution and explicit forwarding still see it.
            assert_eq!(value(TEST_VAR).as_deref(), Some("snapshot-secret"));
            assert!(github_credentials()
                .iter()
                .any(|(name, value)| name == TEST_VAR && value == "snapshot-secret"));
        });
    }

    #[test]
    fn value_prefers_the_live_environment() {
        let _guard = TEST_LOCK.lock().unwrap();
        with_test_env("live-secret", || {
            assert_eq!(value(TEST_VAR).as_deref(), Some("live-secret"));
        });
    }

    #[test]
    fn scrub_without_capture_is_a_no_op() {
        let _guard = TEST_LOCK.lock().unwrap();
        if CAPTURED.get().is_some() {
            return; // another test already captured; scrubbing is active
        }
        with_test_env("still-live", || {
            scrub();
            assert_eq!(value(TEST_VAR).as_deref(), Some("still-live"));
        });
    }
}
