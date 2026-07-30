//! Operator recovery for blocked publication intents, over the admin HTTP surface.
//!
//! The durable factory store is guarded by an exclusive file lock that the
//! orchestrator holds for its whole lifetime, so an external process cannot open
//! the store while Symphony runs — which is exactly when a blocked intent gets
//! discovered. `symphony publication list-blocked` and `reset` therefore ask the
//! running process over HTTP first, and only fall back to opening the store
//! directly when nothing answers.
//!
//! This module holds the client half and every rendering decision, so both paths
//! print the same thing and the whole surface is testable without a CLI process.
//! The routes it targets live in [`crate::http_server`].

use std::fmt;
use std::time::Duration;

use serde::Deserialize;

use crate::http_server::{BlockedPublicationHttpResponse, BlockedPublicationResetHttpResponse};

/// Admin port assumed when neither the workflow config nor `--port` names one.
///
/// Mirrors the orchestrator's own default so the CLI looks where Symphony binds.
pub const DEFAULT_ADMIN_PORT: u16 = 8080;

/// Substring identifying a store-open failure caused by the exclusive lock.
///
/// Produced by `SqliteFactoryStore::acquire_lock_and_migrate`.
pub const STORE_LOCK_MARKER: &str = "could not acquire exclusive triage store lock";

const ADMIN_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Why an admin HTTP call did not produce an answer.
///
/// The distinction drives the CLI's fallback: only [`Self::Unreachable`] means
/// "nothing is listening, so opening the store directly is worth trying".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryHttpError {
    /// Nothing answered at the configured address.
    Unreachable(String),
    /// The orchestrator answered, but the operation did not succeed.
    Failed(String),
}

impl RecoveryHttpError {
    /// Whether falling back to the durable store is worth attempting.
    pub fn is_unreachable(&self) -> bool {
        matches!(self, Self::Unreachable(_))
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Unreachable(message) | Self::Failed(message) => message,
        }
    }
}

impl fmt::Display for RecoveryHttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

/// Which process actually applied a recovery operation.
///
/// Only a running orchestrator reconciles on a poll; a direct-store reset sits
/// until Symphony is next started, and the operator needs to be told which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoverySource {
    RunningOrchestrator,
    DirectStore,
}

/// Build the admin base URL the CLI should call for `host`/`port`.
///
/// Wildcard bind addresses are rewritten to their loopback equivalent: Symphony
/// may bind `0.0.0.0`, but a client has to connect to a concrete address.
pub fn admin_base_url(host: &str, port: u16) -> String {
    let host = match host.trim() {
        "" | "0.0.0.0" => "127.0.0.1",
        "::" | "[::]" | "0:0:0:0:0:0:0:0" => "::1",
        other => other,
    };
    if host.contains(':') && !host.starts_with('[') {
        format!("http://[{host}]:{port}")
    } else {
        format!("http://{host}:{port}")
    }
}

/// Blocked publication intents known to the running orchestrator.
pub async fn fetch_blocked_publications(
    base_url: &str,
) -> Result<Vec<BlockedPublicationHttpResponse>, RecoveryHttpError> {
    #[derive(Deserialize)]
    struct BlockedEnvelope {
        #[serde(default)]
        blocked: Vec<BlockedPublicationHttpResponse>,
    }

    let url = build_url(base_url, &["api", "v1", "publications", "blocked"], &[])?;
    let response = admin_client()?
        .get(url.clone())
        .send()
        .await
        .map_err(|err| classify_request_error(base_url, &err))?;

    if !response.status().is_success() {
        return Err(response_error(&url, response).await);
    }

    response
        .json::<BlockedEnvelope>()
        .await
        .map(|envelope| envelope.blocked)
        .map_err(|err| RecoveryHttpError::Failed(format!("could not read {url}: {err}")))
}

/// Ask the running orchestrator to return a blocked intent to `pending`.
pub async fn reset_blocked_publication(
    base_url: &str,
    intent_id: &str,
    operator: &str,
) -> Result<BlockedPublicationResetHttpResponse, RecoveryHttpError> {
    let url = build_url(
        base_url,
        &["api", "v1", "publications", intent_id, "reset"],
        &[("operator", operator)],
    )?;
    let response = admin_client()?
        .post(url.clone())
        .send()
        .await
        .map_err(|err| classify_request_error(base_url, &err))?;

    if !response.status().is_success() {
        return Err(response_error(&url, response).await);
    }

    response
        .json::<BlockedPublicationResetHttpResponse>()
        .await
        .map_err(|err| RecoveryHttpError::Failed(format!("could not read {url}: {err}")))
}

/// Render the `list-blocked` report.
pub fn format_blocked_publications(blocked: &[BlockedPublicationHttpResponse]) -> String {
    if blocked.is_empty() {
        return "no blocked publication intents".to_string();
    }

    let mut report = format!("{} blocked publication intent(s):", blocked.len());
    for intent in blocked {
        let reason = match (&intent.error_code, &intent.error_remediation) {
            (Some(code), Some(remediation)) => format!("{code} — {remediation}"),
            (Some(code), None) => code.clone(),
            _ => "unknown".to_string(),
        };
        report.push_str(&format!(
            "\n  {}  run={}  retries={}  last_step={}\n    {}",
            intent.intent_id,
            intent.run_id,
            intent.retry_count,
            intent.last_step.as_deref().unwrap_or("none"),
            reason
        ));
    }
    report.push_str("\n\nreset one with: symphony publication reset <intent-id>");
    report
}

/// Render the `reset` confirmation, truthfully for the path that applied it.
pub fn format_publication_reset(
    reset: &BlockedPublicationResetHttpResponse,
    source: RecoverySource,
) -> String {
    let mut message = format!(
        "reset {} to {} (run {}, {} completed step(s) preserved)",
        reset.intent_id,
        reset.status,
        reset.run_id,
        reset.completed_steps.len()
    );
    message.push('\n');
    message.push_str(match source {
        RecoverySource::RunningOrchestrator => {
            "the running orchestrator resumes reconciliation on its next poll; fix the underlying \
             cause first or the intent will exhaust its retries again"
        }
        RecoverySource::DirectStore => {
            "no orchestrator is running, so nothing is reconciling yet; the intent is picked up \
             when Symphony next runs against this workflow. Fix the underlying cause first or the \
             intent will exhaust its retries again"
        }
    });
    message
}

/// Whether a store-open failure was caused by the exclusive lock.
pub fn is_store_lock_error(message: &str) -> bool {
    message.contains(STORE_LOCK_MARKER)
}

/// Explain a recovery attempt that failed over HTTP *and* against the store.
///
/// A bare lock error is baffling here: the lock is held precisely because
/// Symphony is running, so the real fault is that the configured admin address
/// does not point at it. Say that instead of leaking the lock path alone.
pub fn store_unavailable_message(http_error: &RecoveryHttpError, store_error: &str) -> String {
    if http_error.is_unreachable() && is_store_lock_error(store_error) {
        return format!(
            "cannot recover publication intents: the running orchestrator did not answer, and the \
             durable store is locked by another process.\n  http:  {http_error}\n  store: \
             {store_error}\n\nSymphony holds that lock for as long as it runs, so it is running — \
             it just is not reachable at the address above. Check server.host and server.port in \
             the workflow config, or pass --port with the port Symphony was started on."
        );
    }
    format!("{http_error}\n{store_error}")
}

fn admin_client() -> Result<reqwest::Client, RecoveryHttpError> {
    reqwest::Client::builder()
        .timeout(ADMIN_REQUEST_TIMEOUT)
        .build()
        .map_err(|err| RecoveryHttpError::Failed(format!("could not build HTTP client: {err}")))
}

/// Join path segments onto `base_url`, percent-encoding operator-supplied ids.
fn build_url(
    base_url: &str,
    segments: &[&str],
    query: &[(&str, &str)],
) -> Result<reqwest::Url, RecoveryHttpError> {
    let mut url = reqwest::Url::parse(base_url).map_err(|err| {
        RecoveryHttpError::Failed(format!("invalid Symphony admin address {base_url}: {err}"))
    })?;
    url.path_segments_mut()
        .map_err(|_| {
            RecoveryHttpError::Failed(format!(
                "invalid Symphony admin address {base_url}: not a valid base URL"
            ))
        })?
        .clear()
        .extend(segments);
    if !query.is_empty() {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in query {
            pairs.append_pair(key, value);
        }
    }
    Ok(url)
}

/// Flatten an error's source chain into one line.
///
/// `reqwest::Error` renders as "error sending request for url (...)" and keeps
/// the cause an operator actually needs — "Connection refused" — in its source
/// chain, where a bare `{err}` never shows it.
fn error_chain(err: &dyn std::error::Error) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        // hyper repeats its wrapper's text at several levels; keep it readable.
        if !message.contains(&cause_text) {
            message.push_str(&format!(": {cause_text}"));
        }
        source = cause.source();
    }
    message
}

fn classify_request_error(base_url: &str, err: &reqwest::Error) -> RecoveryHttpError {
    let message = format!(
        "could not reach the Symphony admin API at {base_url}: {}",
        error_chain(err)
    );
    if err.is_connect() || err.is_timeout() {
        RecoveryHttpError::Unreachable(message)
    } else {
        RecoveryHttpError::Failed(message)
    }
}

async fn response_error(url: &reqwest::Url, response: reqwest::Response) -> RecoveryHttpError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|payload| {
            payload
                .get("error")?
                .get("message")?
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            let body = body.trim();
            if body.is_empty() {
                status
                    .canonical_reason()
                    .unwrap_or("unknown error")
                    .to_string()
            } else {
                body.to_string()
            }
        });
    RecoveryHttpError::Failed(format!("{url} returned {}: {detail}", status.as_u16()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn blocked_fixture() -> BlockedPublicationHttpResponse {
        BlockedPublicationHttpResponse {
            intent_id: "intent-1".to_string(),
            run_id: "run-1".to_string(),
            kind: "implementation".to_string(),
            retry_count: 3,
            last_step: Some("comment_pending".to_string()),
            error_code: Some("publication_retry_exhausted".to_string()),
            error_remediation: Some("Check the forge token scopes.".to_string()),
            updated_at: Utc
                .with_ymd_and_hms(2026, 7, 30, 12, 0, 0)
                .single()
                .expect("fixture timestamp"),
        }
    }

    #[test]
    fn admin_base_url_rewrites_wildcard_binds_to_loopback() {
        assert_eq!(admin_base_url("0.0.0.0", 8080), "http://127.0.0.1:8080");
        assert_eq!(admin_base_url("  ", 9000), "http://127.0.0.1:9000");
        assert_eq!(admin_base_url("::", 8080), "http://[::1]:8080");
    }

    #[test]
    fn admin_base_url_preserves_concrete_hosts() {
        assert_eq!(admin_base_url("127.0.0.1", 8080), "http://127.0.0.1:8080");
        assert_eq!(
            admin_base_url("symphony.internal", 80),
            "http://symphony.internal:80"
        );
        assert_eq!(admin_base_url("[::1]", 8080), "http://[::1]:8080");
        assert_eq!(admin_base_url("fd00::1", 8080), "http://[fd00::1]:8080");
    }

    #[test]
    fn build_url_percent_encodes_operator_supplied_ids() {
        let url = build_url(
            "http://127.0.0.1:8080",
            &["api", "v1", "publications", "weird id/../x", "reset"],
            &[("operator", "ada lovelace")],
        )
        .expect("url should build");

        assert_eq!(url.path(), "/api/v1/publications/weird%20id%2F..%2Fx/reset");
        assert_eq!(url.query(), Some("operator=ada+lovelace"));
    }

    #[test]
    fn format_blocked_publications_reports_empty_set_plainly() {
        assert_eq!(
            format_blocked_publications(&[]),
            "no blocked publication intents"
        );
    }

    #[test]
    fn format_blocked_publications_names_intent_and_reason() {
        let report = format_blocked_publications(&[blocked_fixture()]);

        assert!(report.starts_with("1 blocked publication intent(s):"));
        assert!(report.contains("intent-1  run=run-1  retries=3  last_step=comment_pending"));
        assert!(report.contains("publication_retry_exhausted — Check the forge token scopes."));
        assert!(report.contains("symphony publication reset <intent-id>"));
    }

    #[test]
    fn format_blocked_publications_falls_back_when_no_error_recorded() {
        let mut intent = blocked_fixture();
        intent.error_code = None;
        intent.error_remediation = None;
        intent.last_step = None;

        let report = format_blocked_publications(&[intent]);

        assert!(report.contains("last_step=none"));
        assert!(report.contains("unknown"));
    }

    #[test]
    fn format_publication_reset_distinguishes_who_applied_it() {
        let reset = BlockedPublicationResetHttpResponse {
            intent_id: "intent-1".to_string(),
            run_id: "run-1".to_string(),
            status: "pending".to_string(),
            completed_steps: vec!["comment_pending".to_string()],
        };

        let served = format_publication_reset(&reset, RecoverySource::RunningOrchestrator);
        assert!(
            served.contains("reset intent-1 to pending (run run-1, 1 completed step(s) preserved)")
        );
        assert!(served.contains("resumes reconciliation on its next poll"));

        let direct = format_publication_reset(&reset, RecoverySource::DirectStore);
        assert!(direct.contains("no orchestrator is running"));
        assert!(!direct.contains("next poll;"));
    }

    #[test]
    fn store_unavailable_message_explains_a_locked_store_after_an_unreachable_api() {
        let http_error = RecoveryHttpError::Unreachable(
            "could not reach the Symphony admin API at http://127.0.0.1:8080: connection refused"
                .to_string(),
        );
        let store_error =
            "could not acquire exclusive triage store lock /data/factory.sqlite3.lock: locked";

        let message = store_unavailable_message(&http_error, store_error);

        assert!(message.contains("server.host and server.port"));
        assert!(message.contains("--port"));
        assert!(message.contains("http://127.0.0.1:8080"));
        assert!(message.contains(store_error));
    }

    #[test]
    fn store_unavailable_message_passes_through_non_lock_failures() {
        let http_error = RecoveryHttpError::Unreachable("nothing answered".to_string());

        let message = store_unavailable_message(&http_error, "no durable factory store at /data");

        assert_eq!(
            message,
            "nothing answered\nno durable factory store at /data"
        );
        assert!(!message.contains("server.host"));
    }

    #[test]
    fn is_store_lock_error_matches_only_the_lock_failure() {
        assert!(is_store_lock_error(
            "storage error: could not acquire exclusive triage store lock /x.lock: locked"
        ));
        assert!(!is_store_lock_error("no durable factory store at /x"));
    }
}
