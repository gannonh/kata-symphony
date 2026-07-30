use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Method, Request, StatusCode};
use chrono::{TimeZone, Utc};
use serde_json::{json, Value};
use symphony::domain::{
    BlockedIssueEntry, CodexTotals, CompletedEntry, EventKind, OrchestratorSnapshot,
    PollingSnapshot, RateLimitInfo, RefreshRequestOutcome, RetrySnapshotEntry, RunAttempt,
    RunningSessionSnapshot, SessionTokenUsage, SharedContextSummary, SupervisorSnapshot,
    SupervisorStatus, WorkerSessionInfo,
};
use symphony::http_server::{
    build_router, parse_event_filter_contract, HttpServerState, RefreshControl, SnapshotSource,
};
use symphony::orchestrator::{steer_channel, SteerResult, SteerSender};
use tower::ServiceExt;

#[derive(Clone)]
struct StaticSnapshotSource {
    snapshot: OrchestratorSnapshot,
}

impl SnapshotSource for StaticSnapshotSource {
    fn snapshot(&self) -> OrchestratorSnapshot {
        self.snapshot.clone()
    }
}

#[derive(Default)]
struct FakeRefreshControl {
    requests: AtomicUsize,
}

impl RefreshControl for FakeRefreshControl {
    fn request_refresh(&self) -> RefreshRequestOutcome {
        let request_idx = self.requests.fetch_add(1, Ordering::SeqCst);
        if request_idx == 0 {
            RefreshRequestOutcome {
                queued: true,
                coalesced: false,
                pending_requests: 1,
            }
        } else {
            RefreshRequestOutcome {
                queued: false,
                coalesced: true,
                pending_requests: 1,
            }
        }
    }
}

fn fixture_snapshot() -> OrchestratorSnapshot {
    let started_at = Utc
        .with_ymd_and_hms(2026, 3, 19, 12, 0, 0)
        .single()
        .expect("fixture timestamp should be valid");

    OrchestratorSnapshot {
        poll_interval_ms: 30_000,
        max_concurrent_agents: 4,
        tracker_project_url: Some("https://linear.app/kata-sh/project/symphony".to_string()),
        running: {
            let mut running = BTreeMap::new();
            running.insert(
                "issue-123".to_string(),
                RunAttempt {
                    issue_id: "issue-123".to_string(),
                    issue_identifier: "SIM-123".to_string(),
                    issue_title: None,
                    attempt: Some(2),
                    workspace_path: "/tmp/symphony/issue-123".to_string(),
                    started_at,
                    status: "running".to_string(),
                    error: None,
                    worker_host: Some("worker-a".to_string()),
                    model: None,
                    tracker_state: None,
                    issue_url: None,
                },
            );
            running
        },
        running_sessions: {
            let mut sessions = BTreeMap::new();
            sessions.insert(
                "issue-123".to_string(),
                RunningSessionSnapshot {
                    turn_count: 2,
                    last_activity_at: Some(started_at),
                    total_tokens: 200,
                    last_event: Some("codex/event/task_started".to_string()),
                    last_event_message: Some("running cargo test".to_string()),
                    session_id: Some("session-12345678".to_string()),
                    current_tool_name: None,
                    current_tool_args_preview: None,
                    last_error: None,
                },
            );
            sessions
        },
        running_session_info: BTreeMap::from([(
            "issue-123".to_string(),
            WorkerSessionInfo {
                turn_count: 3,
                max_turns: 20,
                stall_timeout_ms: 0,
                last_activity_ms: Some(started_at.timestamp_millis() + 70_000),
                session_tokens: SessionTokenUsage {
                    input_tokens: 35,
                    output_tokens: 12,
                    total_tokens: 47,
                },
                current_tool_name: None,
                current_tool_args_preview: None,
                last_error: None,
            },
        )]),
        claimed: BTreeSet::from(["issue-123".to_string()]),
        retry_queue: vec![RetrySnapshotEntry {
            issue_id: "issue-777".to_string(),
            identifier: "SIM-777".to_string(),
            attempt: 3,
            due_in_ms: 9_500,
            error: Some("agent exited: :boom".to_string()),
            worker_host: Some("worker-b".to_string()),
            workspace_path: Some("/tmp/symphony/issue-777".to_string()),
        }],
        completed: vec![CompletedEntry {
            issue_id: "issue-001".to_string(),
            identifier: "KAT-001".to_string(),
            title: "Completed issue".to_string(),
            completed_at: Some(chrono::Utc::now()),
        }],
        pending_escalations: vec![],
        shared_context: SharedContextSummary {
            total_entries: 1,
            entries_by_scope: BTreeMap::from([("project".to_string(), 1)]),
            oldest_entry_at: Some(started_at),
            newest_entry_at: Some(started_at),
        },
        supervisor: symphony::domain::SupervisorSnapshot::default(),
        codex_totals: CodexTotals {
            input_tokens: 120,
            output_tokens: 80,
            total_tokens: 200,
            event_count: 55,
            seconds_running: 42.5,
        },
        blocked: vec![],
        codex_rate_limits: Some(RateLimitInfo {
            data: json!({
                "remaining": 88,
                "limit": 100,
                "reset_at": "2026-03-19T12:05:00Z"
            }),
        }),
        polling: PollingSnapshot {
            checking: false,
            next_poll_in_ms: 5_000,
            poll_interval_ms: 30_000,
            last_poll_at: Some("2026-03-21T12:00:00Z".to_string()),
            poll_count: 42,
        },
        triage_sessions: vec![],
    }
}

fn github_snapshot() -> OrchestratorSnapshot {
    let started_at = Utc
        .with_ymd_and_hms(2026, 3, 19, 12, 0, 0)
        .single()
        .expect("fixture timestamp should be valid");

    let mut snapshot = fixture_snapshot();
    snapshot.tracker_project_url =
        Some("https://github.com/test-owner/test-repo/issues".to_string());
    snapshot.running = BTreeMap::from([(
        "gh-42".to_string(),
        RunAttempt {
            issue_id: "gh-42".to_string(),
            issue_identifier: "#42".to_string(),
            issue_title: Some("GitHub issue parity".to_string()),
            attempt: Some(1),
            workspace_path: "/tmp/symphony/gh-42".to_string(),
            started_at,
            status: "running".to_string(),
            error: None,
            worker_host: Some("worker-a".to_string()),
            model: None,
            tracker_state: Some("In Progress".to_string()),
            issue_url: Some("https://github.com/test-owner/test-repo/issues/42".to_string()),
        },
    )]);
    snapshot.completed = vec![CompletedEntry {
        issue_id: "gh-42".to_string(),
        identifier: "#42".to_string(),
        title: "GitHub issue parity".to_string(),
        completed_at: Some(started_at),
    }];
    snapshot
}

fn test_router() -> axum::Router {
    test_router_with_steer_sender(None)
}

fn test_router_with_steer_sender(steer_sender: Option<SteerSender>) -> axum::Router {
    let state = HttpServerState::new(
        Arc::new(StaticSnapshotSource {
            snapshot: fixture_snapshot(),
        }),
        Arc::new(FakeRefreshControl::default()),
        symphony::orchestrator::EscalationRegistry::default(),
    );

    let state = if let Some(steer_sender) = steer_sender {
        state.with_steer_sender(steer_sender)
    } else {
        state
    };

    build_router(state)
}

fn spawn_steer_response(
    result: SteerResult,
) -> (
    SteerSender,
    tokio::sync::oneshot::Receiver<(String, String)>,
) {
    let (sender, mut receiver) = steer_channel();
    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        if let Some(dispatch) = receiver.recv().await {
            let _ = seen_tx.send((
                dispatch.issue_identifier.clone(),
                dispatch.instruction.clone(),
            ));
            let _ = dispatch.response_tx.send(result);
        }
    });

    (sender, seen_rx)
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body should be readable");
    String::from_utf8(bytes.to_vec()).expect("response body should be utf-8")
}

async fn body_json(response: axum::response::Response) -> Value {
    let text = body_text(response).await;
    serde_json::from_str(&text).expect("response body should be valid JSON")
}

#[tokio::test]
async fn test_get_root_returns_html_dashboard_shell_with_structured_sections() {
    let app = test_router();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();

    assert!(
        content_type.starts_with("text/html"),
        "dashboard endpoint must return HTML content-type"
    );

    let body = body_text(response).await;

    assert!(
        body.contains("Symphony Dashboard"),
        "dashboard shell should include visible product heading"
    );
    assert!(
        body.contains("Running sessions"),
        "dashboard shell should include running sessions table section"
    );
    assert!(
        body.contains("<th>Turn</th>"),
        "running table should include the turn column header"
    );
    assert!(
        body.contains("<th>Last Activity</th>"),
        "running table should include the last activity column header"
    );
    assert!(
        body.contains("<th>Tokens</th>"),
        "running table should include the per-session token column header"
    );
    assert!(
        body.contains("<th>Error</th>"),
        "running table should include the error column header"
    );
    assert!(
        body.contains("stale-activity"),
        "dashboard script should include stale activity highlighting styles/logic"
    );
    assert!(
        body.contains("error-text"),
        "dashboard script should include error styling styles/logic"
    );
    assert!(
        body.contains("sessionInfo.last_error"),
        "dashboard script should consume running-session last_error values"
    );
    assert!(
        body.contains("lastActivityValue != null ? Number(lastActivityValue) : NaN"),
        "dashboard script should treat null last_activity_ms as missing instead of coercing to 0"
    );
    assert!(
        body.contains("Retry queue"),
        "dashboard shell should include retry queue table section"
    );
    assert!(
        body.contains("Shared Context"),
        "dashboard shell should include shared context section"
    );
    assert!(
        body.contains("Supervisor"),
        "dashboard shell should include supervisor section"
    );
    assert!(
        body.contains(r#"id="supervisor-status-detail""#),
        "dashboard shell should include supervisor status detail field"
    );
    assert!(
        body.contains("Completed issues"),
        "dashboard shell should include completed issue list section"
    );
    assert!(
        body.contains("Token summary"),
        "dashboard shell should include token summary section"
    );
    assert!(
        body.contains(r#"id="tracker-project-link""#),
        "dashboard shell should include clickable tracker project link in summary section"
    );
    assert!(
        body.contains("https://linear.app/kata-sh/project/symphony"),
        "dashboard shell should render the configured tracker project URL"
    );
    assert!(
        body.contains("Polling"),
        "dashboard shell should include polling section"
    );
    assert!(
        body.contains("Rate limits"),
        "dashboard shell should include rate-limit diagnostics section"
    );
    assert!(
        body.contains(r#"id="polling-next-poll">n/a"#),
        "dashboard shell should initialize next-poll tile with n/a placeholder"
    );
    assert!(
        body.contains(r#"id="polling-interval">n/a"#),
        "dashboard shell should initialize poll-interval tile with n/a placeholder"
    );
    assert!(
        !body.contains("Live state"),
        "dashboard shell should no longer expose the raw live-state section"
    );
}

#[tokio::test]
async fn test_dashboard_renders_github_identifiers() {
    let app = build_router(HttpServerState::new(
        Arc::new(StaticSnapshotSource {
            snapshot: github_snapshot(),
        }),
        Arc::new(FakeRefreshControl::default()),
        symphony::orchestrator::EscalationRegistry::default(),
    ));

    let dashboard_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    let dashboard_html = body_text(dashboard_response).await;

    assert!(
        dashboard_html.contains("https://github.com/test-owner/test-repo/issues"),
        "dashboard should render GitHub tracker project URL card"
    );
    assert!(
        dashboard_html.contains("buildIssueUrl(issueIdentifier, run.issue_url, trackerProjectUrl)"),
        "running table rendering should resolve issue links using run.issue_url first"
    );
    assert!(
        dashboard_html.contains("if (!projectBase.includes('/issues'))"),
        "running/completed link fallback should only activate for GitHub-style /issues base URLs"
    );
    assert!(
        dashboard_html.contains("projectBase.replace(/\\/+$/, '') + '/' + issueNumber"),
        "running/completed link rendering should append numeric issue ids to GitHub issue base URLs"
    );

    let state_response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/state")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    let payload = body_json(state_response).await;
    assert_eq!(payload["running"]["gh-42"]["issue_identifier"], "#42");
    assert_eq!(
        payload["running"]["gh-42"]["issue_url"],
        "https://github.com/test-owner/test-repo/issues/42"
    );
    assert_eq!(
        payload["tracker_project_url"],
        "https://github.com/test-owner/test-repo/issues"
    );
}

#[tokio::test]
async fn test_dashboard_does_not_fallback_issue_links_for_linear_project_urls() {
    let app = test_router();

    let dashboard_response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    let dashboard_html = body_text(dashboard_response).await;

    assert!(
        dashboard_html.contains("if (!projectBase.includes('/issues'))"),
        "dashboard JS should guard numeric issue-url fallback so Linear project URLs are not treated like issue bases"
    );
    assert!(
        !dashboard_html.contains("trackerProjectUrl.replace(/\\/+$/, '') + '/' + issueNumber"),
        "dashboard JS should no longer append issue numbers to arbitrary tracker project URLs"
    );
}

#[tokio::test]
async fn test_dashboard_initial_supervisor_metrics_use_snapshot_values() {
    let mut snapshot = fixture_snapshot();
    let last_action_at = Utc
        .with_ymd_and_hms(2026, 3, 22, 9, 15, 0)
        .single()
        .expect("fixture timestamp should be valid");

    snapshot.supervisor = SupervisorSnapshot {
        status: SupervisorStatus::Active,
        model: Some("anthropic/claude-sonnet-4-6".to_string()),
        steers_issued: 7,
        conflicts_detected: 3,
        patterns_detected: 2,
        escalations_created: 1,
        last_decision: Some("steered KAT-1327 (no_progress)".to_string()),
        last_action_at: Some(last_action_at),
        last_error: None,
    };

    let app = build_router(HttpServerState::new(
        Arc::new(StaticSnapshotSource { snapshot }),
        Arc::new(FakeRefreshControl::default()),
        symphony::orchestrator::EscalationRegistry::default(),
    ));

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    let body = body_text(response).await;

    assert!(
        body.contains(r#"id="supervisor-steers">7"#),
        "dashboard should server-render supervisor steers"
    );
    assert!(
        body.contains(r#"id="supervisor-conflicts">3"#),
        "dashboard should server-render supervisor conflicts"
    );
    assert!(
        body.contains(r#"id="supervisor-patterns">2"#),
        "dashboard should server-render supervisor patterns"
    );
    assert!(
        body.contains(r#"id="supervisor-escalations">1"#),
        "dashboard should server-render supervisor escalations"
    );
    assert!(
        body.contains(r#"id="supervisor-last-decision">steered KAT-1327 (no_progress)"#),
        "dashboard should server-render supervisor last decision"
    );
    assert!(
        body.contains(r#"id="supervisor-last-action">2026-03-22T09:15:00+00:00"#),
        "dashboard should server-render supervisor last action timestamp"
    );
}

#[tokio::test]
async fn test_dashboard_html_includes_error_column_rendering_logic() {
    let mut snapshot = fixture_snapshot();
    let issue_id = "issue-123".to_string();
    let session_info = snapshot
        .running_session_info
        .get_mut(&issue_id)
        .expect("fixture running session info should include issue-123");
    session_info.last_error = Some("You have hit your ChatGPT usage limit".to_string());

    let app = build_router(HttpServerState::new(
        Arc::new(StaticSnapshotSource { snapshot }),
        Arc::new(FakeRefreshControl::default()),
        symphony::orchestrator::EscalationRegistry::default(),
    ));

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    let body = body_text(response).await;

    assert!(
        body.contains("<th>Error</th>"),
        "dashboard should expose the error column header"
    );
    assert!(
        body.contains("<td class=\"mono error-text\">"),
        "dashboard script should render an error-text table cell when last_error is present"
    );
    assert!(
        body.contains("<td class=\"muted\">-</td>"),
        "dashboard script should render muted fallback when last_error is absent"
    );
    assert!(
        body.contains("colspan=\"13\""),
        "running empty state should reserve the extra error column"
    );
}

#[tokio::test]
async fn test_get_api_state_includes_worker_last_error_when_present() {
    let mut snapshot = fixture_snapshot();
    let issue_id = "issue-123".to_string();
    let session_info = snapshot
        .running_session_info
        .get_mut(&issue_id)
        .expect("fixture running session info should include issue-123");
    session_info.last_error = Some("You have hit your ChatGPT usage limit".to_string());

    let app = build_router(HttpServerState::new(
        Arc::new(StaticSnapshotSource { snapshot }),
        Arc::new(FakeRefreshControl::default()),
        symphony::orchestrator::EscalationRegistry::default(),
    ));

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/state")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    let payload = body_json(response).await;

    assert_eq!(
        payload["running_session_info"]["issue-123"]["last_error"],
        "You have hit your ChatGPT usage limit"
    );
}

#[tokio::test]
async fn test_get_api_state_returns_snapshot_projection() {
    let app = test_router();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/state")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);

    let payload = body_json(response).await;

    assert_eq!(
        payload["running"]["issue-123"]["issue_identifier"],
        "SIM-123"
    );
    assert_eq!(
        payload["running_session_info"]["issue-123"]["turn_count"],
        3
    );
    assert_eq!(
        payload["running_session_info"]["issue-123"]["session_tokens"]["total_tokens"],
        47
    );
    assert_eq!(
        payload["running_sessions"]["issue-123"]["last_event"],
        "codex/event/task_started"
    );
    assert_eq!(
        payload["running_sessions"]["issue-123"]["last_event_message"],
        "running cargo test"
    );
    assert_eq!(
        payload["running_sessions"]["issue-123"]["session_id"],
        "session-12345678"
    );
    assert_eq!(payload["retry_queue"][0]["identifier"], "SIM-777");
    assert_eq!(payload["shared_context"]["total_entries"], 1);
    assert_eq!(payload["supervisor"]["status"], "disabled");
    assert_eq!(payload["codex_totals"]["total_tokens"], 200);
    assert_eq!(payload["codex_rate_limits"]["remaining"], 88);
    assert_eq!(
        payload["tracker_project_url"],
        "https://linear.app/kata-sh/project/symphony"
    );
    assert_eq!(payload["polling"]["next_poll_in_ms"], 5_000);
}

#[tokio::test]
async fn test_get_issue_returns_projection_for_known_issue_identifier() {
    let app = test_router();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/SIM-123")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);

    let payload = body_json(response).await;

    assert_eq!(payload["issue"]["issue_identifier"], "SIM-123");
    assert_eq!(payload["issue"]["issue_id"], "issue-123");
    assert_eq!(payload["issue"]["status"], "running");
}

#[tokio::test]
async fn test_get_issue_returns_not_found_envelope_for_unknown_identifier() {
    let app = test_router();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/SIM-999")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let payload = body_json(response).await;

    assert_eq!(payload["error"]["code"], "issue_not_found");
    assert_eq!(payload["error"]["status"], 404);
    assert!(
        payload["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("SIM-999"),
        "issue-not-found message should include requested identifier"
    );
}

#[tokio::test]
async fn test_post_refresh_reports_queued_then_coalesced_state() {
    let app = test_router();

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/refresh")
                .body(Body::empty())
                .expect("first request should build"),
        )
        .await
        .expect("router should respond to first refresh");

    assert_eq!(first.status(), StatusCode::ACCEPTED);
    let first_payload = body_json(first).await;
    assert_eq!(first_payload["queued"], true);
    assert_eq!(first_payload["coalesced"], false);
    assert_eq!(first_payload["pending_requests"], 1);

    let second = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/refresh")
                .body(Body::empty())
                .expect("second request should build"),
        )
        .await
        .expect("router should respond to second refresh");

    assert_eq!(second.status(), StatusCode::ACCEPTED);
    let second_payload = body_json(second).await;
    assert_eq!(second_payload["queued"], false);
    assert_eq!(second_payload["coalesced"], true);
    assert_eq!(second_payload["pending_requests"], 1);
}

#[tokio::test]
async fn test_context_post_get_and_delete_round_trip() {
    let app = test_router();

    let post_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/context")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "author_issue": "KAT-920",
                        "scope": "project",
                        "content": "Decision: use zod schemas",
                    }))
                    .expect("request body should serialize"),
                ))
                .expect("request should build"),
        )
        .await
        .expect("router should respond to context post");

    assert_eq!(post_response.status(), StatusCode::CREATED);
    let post_payload = body_json(post_response).await;
    let entry_id = post_payload["id"]
        .as_str()
        .expect("response should include entry id")
        .to_string();

    let get_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/context")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond to context get");

    assert_eq!(get_response.status(), StatusCode::OK);
    let get_payload = body_json(get_response).await;
    let entries = get_payload["entries"]
        .as_array()
        .expect("entries should be an array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["id"], entry_id);
    assert_eq!(entries[0]["author_issue"], "KAT-920");
    assert_eq!(entries[0]["scope"]["type"], "project");

    let delete_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(&format!("/api/v1/context/{entry_id}"))
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond to context delete");

    assert_eq!(delete_response.status(), StatusCode::OK);
    let delete_payload = body_json(delete_response).await;
    assert_eq!(delete_payload["deleted"], 1);

    let get_after_delete = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/context")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond to context get");

    let payload_after_delete = body_json(get_after_delete).await;
    assert_eq!(
        payload_after_delete["entries"]
            .as_array()
            .expect("entries should remain an array")
            .len(),
        0
    );
}

#[tokio::test]
async fn test_context_scope_filter_and_clear_endpoint() {
    let app = test_router();

    for payload in [
        json!({
            "author_issue": "KAT-920",
            "scope": "project",
            "content": "Global decision",
        }),
        json!({
            "author_issue": "KAT-921",
            "scope": "label:backend",
            "content": "Backend-specific decision",
        }),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/context")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&payload).expect("request body should serialize"),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("router should respond");
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    let filtered = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/context?scope=label:backend")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    assert_eq!(filtered.status(), StatusCode::OK);
    let filtered_payload = body_json(filtered).await;
    assert_eq!(
        filtered_payload["entries"]
            .as_array()
            .expect("entries should be an array")
            .len(),
        1
    );
    assert_eq!(filtered_payload["entries"][0]["scope"]["value"], "backend");

    let cleared = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/v1/context?scope=project")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    assert_eq!(cleared.status(), StatusCode::OK);
    let cleared_payload = body_json(cleared).await;
    assert_eq!(cleared_payload["deleted"], 1);

    let remaining = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/context?scope=label:backend")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    assert_eq!(remaining.status(), StatusCode::OK);
    let remaining_payload = body_json(remaining).await;
    assert_eq!(
        remaining_payload["entries"]
            .as_array()
            .expect("entries should be an array")
            .len(),
        1,
        "scoped clear should preserve non-matching entries"
    );
}

#[tokio::test]
async fn test_context_post_publishes_shared_context_written_event() {
    let state = HttpServerState::new(
        Arc::new(StaticSnapshotSource {
            snapshot: fixture_snapshot(),
        }),
        Arc::new(FakeRefreshControl::default()),
        symphony::orchestrator::EscalationRegistry::default(),
    );
    let mut events = state.event_hub().subscribe();
    let app = build_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/context")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "author_issue": "KAT-921",
                        "scope": "label:backend",
                        "content": "Pattern: keep schema in one module",
                    }))
                    .expect("request body should serialize"),
                ))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::CREATED);

    let envelope = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
        .await
        .expect("event should arrive before timeout")
        .expect("event should decode");

    assert_eq!(envelope.kind, EventKind::SharedContextWritten);
    assert_eq!(envelope.event, "shared_context_written");
    assert_eq!(envelope.payload["author_issue"], "KAT-921");
    assert_eq!(envelope.payload["scope"], "label:backend");
}

#[test]
fn test_event_filter_invalid_type_returns_machine_readable_error() {
    let err = parse_event_filter_contract(None, Some("worker,wat"), None)
        .expect_err("unknown event type should fail");

    assert_eq!(err.field, "type");
    assert_eq!(err.value, "wat");
    let allowed_values = format!("Allowed values: {}", EventKind::variants().join(","));
    assert!(
        err.message.contains(&allowed_values),
        "error should list deterministic allowed values"
    );
}

#[test]
fn test_event_filter_issue_requires_team_number_shape() {
    let err = parse_event_filter_contract(Some("KAT--1"), None, None)
        .expect_err("malformed issue identifier should fail");

    assert_eq!(err.field, "issue");
    assert_eq!(err.value, "KAT--1");
    assert!(
        err.message.contains("expected TEAM-123 style identifier"),
        "error should explain required issue identifier shape"
    );
}

#[test]
fn test_event_filter_issue_normalizes_valid_identifier() {
    let filter = parse_event_filter_contract(Some("kat-1149"), None, None)
        .expect("valid issue identifier should parse");

    assert_eq!(
        filter.issues,
        BTreeSet::from(["KAT-1149".to_string()]),
        "issue filters should be normalized to uppercase"
    );
}

#[tokio::test]
async fn test_unknown_api_path_returns_json_404_error_envelope() {
    let app = test_router();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/does-not-exist")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let payload = body_json(response).await;

    assert_eq!(payload["error"]["code"], "not_found");
    assert_eq!(payload["error"]["status"], 404);
    assert!(
        payload["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("/api/v1/does-not-exist"),
        "404 envelope should include unmatched path"
    );
}

#[tokio::test]
async fn test_wrong_method_on_known_api_route_returns_json_405_error_envelope() {
    let app = test_router();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/refresh")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);

    let payload = body_json(response).await;

    assert_eq!(payload["error"]["code"], "method_not_allowed");
    assert_eq!(payload["error"]["status"], 405);
    assert!(
        payload["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("GET"),
        "405 envelope should include rejected method"
    );
}

#[tokio::test]
async fn test_dashboard_html_includes_blocked_section() {
    let mut snapshot = fixture_snapshot();
    snapshot.blocked = vec![BlockedIssueEntry {
        issue_id: "issue-blocked-1".to_string(),
        identifier: "SIM-100".to_string(),
        title: "Blocked task".to_string(),
        state: "Todo".to_string(),
        blocker_identifiers: vec!["SIM-99".to_string()],
    }];

    let source = StaticSnapshotSource { snapshot };
    let state = HttpServerState::new(
        Arc::new(source),
        Arc::new(FakeRefreshControl::default()),
        symphony::orchestrator::EscalationRegistry::default(),
    );
    let app = build_router(state);

    let req = Request::builder().uri("/").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let html = String::from_utf8_lossy(&body);
    assert!(
        html.contains("Blocked issues"),
        "dashboard HTML should contain blocked issues section"
    );
}

#[tokio::test]
async fn test_state_json_includes_blocked_array() {
    let mut snapshot = fixture_snapshot();
    snapshot.blocked = vec![BlockedIssueEntry {
        issue_id: "issue-blocked-2".to_string(),
        identifier: "SIM-200".to_string(),
        title: "Another blocked".to_string(),
        state: "In Progress".to_string(),
        blocker_identifiers: vec!["SIM-198".to_string(), "SIM-199".to_string()],
    }];

    let source = StaticSnapshotSource { snapshot };
    let state = HttpServerState::new(
        Arc::new(source),
        Arc::new(FakeRefreshControl::default()),
        symphony::orchestrator::EscalationRegistry::default(),
    );
    let app = build_router(state);

    let req = Request::builder()
        .uri("/api/v1/state")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();

    let blocked = payload["blocked"]
        .as_array()
        .expect("blocked should be an array");
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0]["identifier"], "SIM-200");
    assert_eq!(
        blocked[0]["blocker_identifiers"].as_array().unwrap().len(),
        2
    );
}

#[tokio::test]
async fn test_escalation_dashboard_section_renders() {
    let app = test_router();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_text(response).await;
    assert!(body.contains("Pending Escalations"));
    assert!(body.contains("escalation-table-body"));
}

#[tokio::test]
async fn test_escalation_endpoints_return_empty_or_not_found_when_unknown() {
    let app = test_router();

    let list_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/escalations")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(list_response.status(), StatusCode::OK);
    let list_payload = body_json(list_response).await;
    assert_eq!(list_payload, json!({"pending": []}));

    let respond_response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/escalations/missing/respond")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"response": {"confirmed": true}}).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(respond_response.status(), StatusCode::NOT_FOUND);
    let respond_payload = body_json(respond_response).await;
    assert_eq!(respond_payload, json!({"error": "escalation_not_found"}));
}

#[tokio::test]
async fn test_steer_endpoint_returns_404_for_unknown_issue() {
    let (steer_sender, seen_rx) = spawn_steer_response(SteerResult::IssueNotRunning);
    let app = test_router_with_steer_sender(Some(steer_sender));

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/steer")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"issue_identifier": "SIM-404", "instruction": "check logs"}).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let payload = body_json(response).await;
    assert_eq!(payload["error"]["code"], "issue_not_running");

    let (issue_identifier, instruction) =
        tokio::time::timeout(std::time::Duration::from_secs(1), seen_rx)
            .await
            .expect("steer dispatch should be observed")
            .expect("dispatch payload should be captured");

    assert_eq!(issue_identifier, "SIM-404");
    assert_eq!(instruction, "check logs");
}

#[tokio::test]
async fn test_steer_endpoint_returns_200_for_running_issue() {
    let (steer_sender, seen_rx) = spawn_steer_response(SteerResult::Delivered {
        issue_id: "issue-123".to_string(),
        issue_identifier: "SIM-123".to_string(),
    });
    let app = test_router_with_steer_sender(Some(steer_sender));

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/steer")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "issue_identifier": "sim-123",
                        "instruction": "Use the existing auth module"
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response).await;
    assert_eq!(payload["ok"], true);
    assert_eq!(payload["issue_id"], "issue-123");
    assert_eq!(payload["issue_identifier"], "SIM-123");
    assert_eq!(payload["delivered"], true);
    assert_eq!(
        payload["instruction_preview"],
        "Use the existing auth module"
    );

    let (issue_identifier, instruction) =
        tokio::time::timeout(std::time::Duration::from_secs(1), seen_rx)
            .await
            .expect("steer dispatch should be observed")
            .expect("dispatch payload should be captured");

    assert_eq!(issue_identifier, "SIM-123");
    assert_eq!(instruction, "Use the existing auth module");
}

#[tokio::test]
async fn test_steer_endpoint_validates_request_body() {
    let app = test_router_with_steer_sender(None);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/steer")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"issue_identifier": "SIM-123"}).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload = body_json(response).await;
    assert_eq!(payload["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn test_steer_endpoint_instruction_too_long() {
    let app = test_router_with_steer_sender(None);
    let instruction = "x".repeat(5_001);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/steer")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "issue_identifier": "SIM-123",
                        "instruction": instruction
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let payload = body_json(response).await;
    assert_eq!(payload["error"]["code"], "instruction_too_long");
}

#[derive(Default)]
struct FakeFactoryRunQuery {
    by_id: BTreeMap<String, symphony::http_server::FactoryRunHttpResponse>,
    by_issue: BTreeMap<String, symphony::http_server::FactoryRunHttpResponse>,
    metrics: Option<symphony::http_server::FactoryRunMetricsHttpResponse>,
    spec_metrics: Option<symphony::http_server::SpecRunMetricsHttpResponse>,
    /// `None` leaves the trait default in place, standing in for an implementor
    /// that does not serve publication recovery at all.
    blocked_publications: Option<Vec<symphony::http_server::BlockedPublicationHttpResponse>>,
    /// Keyed by intent id; `Err` carries the store's message so the route's
    /// status-code mapping can be exercised.
    resets: BTreeMap<
        String,
        Result<symphony::http_server::BlockedPublicationResetHttpResponse, String>,
    >,
    /// Records the operator each reset was attributed to.
    reset_operators: Arc<std::sync::Mutex<Vec<(String, String)>>>,
}

impl symphony::http_server::FactoryRunQuery for FakeFactoryRunQuery {
    fn get_run(
        &self,
        run_id: &str,
    ) -> Result<Option<symphony::http_server::FactoryRunHttpResponse>, String> {
        Ok(self.by_id.get(run_id).cloned())
    }

    fn get_run_by_issue(
        &self,
        issue_identifier: &str,
    ) -> Result<Option<symphony::http_server::FactoryRunHttpResponse>, String> {
        Ok(self.by_issue.get(issue_identifier).cloned())
    }

    fn triage_metrics(
        &self,
    ) -> Result<symphony::http_server::FactoryRunMetricsHttpResponse, String> {
        self.metrics
            .clone()
            .ok_or_else(|| "metrics unavailable".to_string())
    }

    fn spec_metrics(&self) -> Result<symphony::http_server::SpecRunMetricsHttpResponse, String> {
        self.spec_metrics
            .clone()
            .ok_or_else(|| "spec metrics unavailable".to_string())
    }

    fn blocked_publications(
        &self,
    ) -> Result<Vec<symphony::http_server::BlockedPublicationHttpResponse>, String> {
        match &self.blocked_publications {
            Some(blocked) => Ok(blocked.clone()),
            None => Err("blocked publications unavailable".to_string()),
        }
    }

    fn reset_blocked_publication(
        &self,
        intent_id: &str,
        operator: &str,
    ) -> Result<symphony::http_server::BlockedPublicationResetHttpResponse, String> {
        self.reset_operators
            .lock()
            .expect("reset operators lock")
            .push((intent_id.to_string(), operator.to_string()));
        self.resets.get(intent_id).cloned().unwrap_or_else(|| {
            Err(format!(
                "implementation publication intent {intent_id} not found"
            ))
        })
    }
}

fn sample_blocked_publication() -> symphony::http_server::BlockedPublicationHttpResponse {
    symphony::http_server::BlockedPublicationHttpResponse {
        intent_id: "intent-blocked-1".to_string(),
        run_id: "run-7".to_string(),
        kind: "draft_pr".to_string(),
        retry_count: 5,
        last_step: Some("comment_pending".to_string()),
        error_code: Some("publication_retry_exhausted".to_string()),
        error_remediation: Some("Restore forge write access, then reset.".to_string()),
        updated_at: Utc
            .with_ymd_and_hms(2026, 7, 30, 9, 15, 0)
            .single()
            .expect("timestamp"),
    }
}

fn sample_publication_reset() -> symphony::http_server::BlockedPublicationResetHttpResponse {
    symphony::http_server::BlockedPublicationResetHttpResponse {
        intent_id: "intent-blocked-1".to_string(),
        run_id: "run-7".to_string(),
        status: "pending".to_string(),
        completed_steps: vec!["comment_pending".to_string()],
    }
}

fn sample_factory_run() -> symphony::http_server::FactoryRunHttpResponse {
    let started = Utc
        .with_ymd_and_hms(2026, 7, 16, 17, 0, 1)
        .single()
        .expect("timestamp");
    let completed = Utc
        .with_ymd_and_hms(2026, 7, 16, 17, 0, 31)
        .single()
        .expect("timestamp");
    symphony::http_server::FactoryRunHttpResponse {
        run_id: "run-1".to_string(),
        forge_host: "github.com".to_string(),
        repository: "example/widgets".to_string(),
        issue: symphony::http_server::FactoryRunIssueHttp {
            id: "123".to_string(),
            identifier: "#123".to_string(),
            revision: Some("abc".to_string()),
        },
        status: "active".to_string(),
        current_stage: Some("triage".to_string()),
        created_at: started,
        updated_at: completed,
        attempts: vec![symphony::http_server::FactoryRunAttemptHttp {
            stage_run_id: "stage-1".to_string(),
            stage: "triage".to_string(),
            attempt: 1,
            status: "completed".to_string(),
            configuration_revision: "cfg".to_string(),
            harness: "pi".to_string(),
            model: Some("anthropic/claude-sonnet-4-6".to_string()),
            started_at: Some(started),
            completed_at: Some(completed),
            duration_ms: Some(30_000),
            usage: symphony::triage::domain::StageUsage {
                input_tokens: 1000,
                output_tokens: 250,
                total_tokens: 1250,
            },
            error: None,
            turns: vec![],
        }],
        artifact: Some(symphony::http_server::FactoryRunArtifactHttp {
            artifact_id: "art-1".to_string(),
            schema_version: 1,
            route: "implement".to_string(),
            risk_class: "low".to_string(),
            rationale: "Bounded docs fix.".to_string(),
            evidence: vec![symphony::triage::domain::TriageEvidence {
                kind: symphony::triage::domain::EvidenceKind::Issue,
                reference: "body".to_string(),
                summary: "Names the file.".to_string(),
            }],
            next_action: "Apply the fix.".to_string(),
            clarification_question: None,
            reproduction: None,
            received_at: completed,
        }),
        publication: Some(symphony::http_server::FactoryRunPublicationHttp {
            intent_id: "intent-1".to_string(),
            mode: "preview".to_string(),
            status: "pending".to_string(),
            completed_steps: vec!["comment_pending".to_string()],
            route_label: "ready-for-agent".to_string(),
            project_state: Some("Todo".to_string()),
            retry_count: 0,
            error: None,
        }),
        spec: None,
        implementation: None,
    }
}

fn router_with_factory_query(query: FakeFactoryRunQuery) -> axum::Router {
    let state = HttpServerState::new(
        Arc::new(StaticSnapshotSource {
            snapshot: fixture_snapshot(),
        }),
        Arc::new(FakeRefreshControl::default()),
        symphony::orchestrator::EscalationRegistry::default(),
    )
    .with_factory_run_query(Arc::new(query));
    build_router(state)
}

#[tokio::test]
async fn test_factory_runs_unavailable_without_query_source() {
    let app = build_router(HttpServerState::new(
        Arc::new(StaticSnapshotSource {
            snapshot: fixture_snapshot(),
        }),
        Arc::new(FakeRefreshControl::default()),
        symphony::orchestrator::EscalationRegistry::default(),
    ));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/factory-runs/run-1")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let payload = body_json(response).await;
    assert_eq!(payload["error"]["code"], "factory_store_unavailable");
}

#[tokio::test]
async fn test_factory_run_by_id_404_and_200() {
    let run = sample_factory_run();
    let mut by_id = BTreeMap::new();
    by_id.insert(run.run_id.clone(), run.clone());
    let app = router_with_factory_query(FakeFactoryRunQuery {
        by_id,
        ..Default::default()
    });

    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/factory-runs/missing")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let missing_payload = body_json(missing).await;
    assert_eq!(missing_payload["error"]["code"], "factory_run_not_found");

    let found = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/factory-runs/run-1")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(found.status(), StatusCode::OK);
    let payload = body_json(found).await;
    assert_eq!(payload["run_id"], "run-1");
    assert_eq!(payload["issue"]["identifier"], "#123");
    assert_eq!(payload["attempts"][0]["duration_ms"], 30000);
    assert_eq!(payload["artifact"]["route"], "implement");
    assert_eq!(payload["publication"]["mode"], "preview");
}

#[tokio::test]
async fn test_factory_runs_issue_query_validation_and_lookup() {
    let run = sample_factory_run();
    let mut by_issue = BTreeMap::new();
    by_issue.insert("#123".to_string(), run);
    let app = router_with_factory_query(FakeFactoryRunQuery {
        by_issue,
        ..Default::default()
    });

    let missing_param = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/factory-runs")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(missing_param.status(), StatusCode::BAD_REQUEST);

    let invalid = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/factory-runs?issue=")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let not_found = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/factory-runs?issue=%23456")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(not_found.status(), StatusCode::OK);
    let empty_payload = body_json(not_found).await;
    assert!(empty_payload.is_null());

    let found = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/factory-runs?issue=%23123")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(found.status(), StatusCode::OK);
    let payload = body_json(found).await;
    assert_eq!(payload["run_id"], "run-1");
}

#[tokio::test]
async fn test_factory_run_metrics_stage_validation() {
    let metrics = symphony::http_server::FactoryRunMetricsHttpResponse {
        stage: "triage".to_string(),
        total_attempts: 2,
        completed_attempts: 1,
        failed_attempts: 1,
        ineligible_issues: 0,
        route_counts: BTreeMap::from([("implement".to_string(), 1)]),
        correction_count: 0,
        correction_rate: 0.0,
        duration: symphony::triage::domain::TriageMetricsDuration {
            average_ms: Some(1000.0),
            p50_ms: Some(1000.0),
            p95_ms: Some(1000.0),
        },
        tokens_by_harness_model: BTreeMap::from([(
            "pi/unknown".to_string(),
            symphony::triage::domain::TriageMetricsTokenTotals {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
            },
        )]),
    };
    let app = router_with_factory_query(FakeFactoryRunQuery {
        metrics: Some(metrics.clone()),
        ..Default::default()
    });

    let bad_stage = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/factory-runs/metrics?stage=review")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(bad_stage.status(), StatusCode::BAD_REQUEST);

    let missing_stage = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/factory-runs/metrics")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(missing_stage.status(), StatusCode::BAD_REQUEST);

    let ok = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/factory-runs/metrics?stage=triage")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(ok.status(), StatusCode::OK);
    let payload = body_json(ok).await;
    assert_eq!(payload["stage"], "triage");
    assert_eq!(payload["total_attempts"], 2);
    assert_eq!(payload["route_counts"]["implement"], 1);

    // Criterion 11 requires the A2-specific aggregates on stage=spec. Without them
    // operators cannot measure review-loop quality or approval latency.
    let app = router_with_factory_query(FakeFactoryRunQuery {
        metrics: Some(metrics),
        spec_metrics: Some(symphony::http_server::SpecRunMetricsHttpResponse {
            base: symphony::http_server::FactoryRunMetricsHttpResponse {
                stage: "spec".to_string(),
                total_attempts: 4,
                completed_attempts: 3,
                failed_attempts: 1,
                ineligible_issues: 2,
                route_counts: BTreeMap::new(),
                correction_count: 0,
                correction_rate: 0.0,
                duration: symphony::triage::domain::TriageMetricsDuration {
                    average_ms: Some(5000.0),
                    p50_ms: Some(4000.0),
                    p95_ms: Some(9000.0),
                },
                tokens_by_harness_model: BTreeMap::from([(
                    "pi/model-a".to_string(),
                    symphony::triage::domain::TriageMetricsTokenTotals {
                        input_tokens: 1000,
                        output_tokens: 100,
                        total_tokens: 1100,
                    },
                )]),
            },
            review_cycles: symphony::spec::domain::SpecReviewCycleMetrics {
                average: Some(1.5),
                max: Some(3),
            },
            converged_attempts: 2,
            convergence_rate: 0.666,
            revision_requests: 1,
            approval_latency: symphony::triage::domain::TriageMetricsDuration {
                average_ms: Some(120000.0),
                p50_ms: Some(120000.0),
                p95_ms: Some(120000.0),
            },
        }),
        ..Default::default()
    });
    let spec = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/factory-runs/metrics?stage=spec")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(spec.status(), StatusCode::OK);
    let payload = body_json(spec).await;
    assert_eq!(payload["stage"], "spec");
    assert_eq!(payload["total_attempts"], 4);
    assert_eq!(payload["review_cycles"]["average"], 1.5);
    assert_eq!(payload["review_cycles"]["max"], 3);
    assert_eq!(payload["converged_attempts"], 2);
    assert_eq!(payload["revision_requests"], 1);
    assert_eq!(payload["approval_latency"]["average_ms"], 120000.0);
    assert_eq!(
        payload["tokens_by_harness_model"]["pi/model-a"]["total_tokens"],
        1100
    );
}

// ── Publication recovery: routes and the CLI client that calls them ────
//
// The orchestrator holds an exclusive lock on the durable store for its whole
// lifetime, so `symphony publication list-blocked` / `reset` cannot open the
// store while Symphony runs — the exact moment an operator needs them. These
// routes serve recovery from the process that owns the store, and the tests
// below cover both the routes and the client in `publication_recovery`.

/// Serve `app` on an ephemeral loopback port and return its base URL.
async fn serve_router(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral port should bind");
    let port = listener.local_addr().expect("local addr").port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://127.0.0.1:{port}")
}

/// A loopback base URL with nothing listening on it.
async fn unused_base_url() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("ephemeral port should bind");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

#[tokio::test]
async fn test_blocked_publications_route_returns_the_blocked_set() {
    let app = router_with_factory_query(FakeFactoryRunQuery {
        blocked_publications: Some(vec![sample_blocked_publication()]),
        ..Default::default()
    });

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/publications/blocked")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response).await;
    assert_eq!(payload["blocked"][0]["intent_id"], "intent-blocked-1");
    assert_eq!(payload["blocked"][0]["retry_count"], 5);
    assert_eq!(payload["blocked"][0]["last_step"], "comment_pending");
    assert_eq!(
        payload["blocked"][0]["error_code"],
        "publication_retry_exhausted"
    );
}

#[tokio::test]
async fn test_blocked_publications_route_reports_an_unavailable_implementor() {
    let app = router_with_factory_query(FakeFactoryRunQuery::default());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/publications/blocked")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let payload = body_json(response).await;
    assert_eq!(payload["error"]["code"], "blocked_publications_unavailable");
}

#[tokio::test]
async fn test_publication_reset_route_records_the_operator() {
    let operators = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = router_with_factory_query(FakeFactoryRunQuery {
        resets: BTreeMap::from([(
            "intent-blocked-1".to_string(),
            Ok(sample_publication_reset()),
        )]),
        reset_operators: Arc::clone(&operators),
        ..Default::default()
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/publications/intent-blocked-1/reset?operator=ada")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response).await;
    assert_eq!(payload["status"], "pending");
    assert_eq!(payload["completed_steps"][0], "comment_pending");
    assert_eq!(
        operators.lock().expect("operators lock").as_slice(),
        [("intent-blocked-1".to_string(), "ada".to_string())]
    );
}

#[tokio::test]
async fn test_publication_reset_route_defaults_an_absent_operator() {
    let operators = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = router_with_factory_query(FakeFactoryRunQuery {
        resets: BTreeMap::from([(
            "intent-blocked-1".to_string(),
            Ok(sample_publication_reset()),
        )]),
        reset_operators: Arc::clone(&operators),
        ..Default::default()
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/publications/intent-blocked-1/reset")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        operators.lock().expect("operators lock")[0].1,
        "unknown".to_string()
    );
}

#[tokio::test]
async fn test_publication_reset_route_maps_store_errors_to_status_codes() {
    // An operator mistake and a fault must not look alike to a client: only the
    // latter is worth retrying.
    let app = router_with_factory_query(FakeFactoryRunQuery {
        resets: BTreeMap::from([
            (
                "missing".to_string(),
                Err("implementation publication intent missing not found".to_string()),
            ),
            (
                "already-pending".to_string(),
                Err("intent already-pending is pending, not blocked".to_string()),
            ),
            (
                "raced".to_string(),
                Err("intent raced changed status concurrently; re-run list-blocked".to_string()),
            ),
            (
                "broken".to_string(),
                Err("storage error: disk went away".to_string()),
            ),
        ]),
        ..Default::default()
    });

    let cases = [
        ("missing", StatusCode::NOT_FOUND),
        ("already-pending", StatusCode::CONFLICT),
        ("raced", StatusCode::CONFLICT),
        ("broken", StatusCode::INTERNAL_SERVER_ERROR),
    ];

    for (intent_id, expected) in cases {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/v1/publications/{intent_id}/reset"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), expected, "intent {intent_id}");
        let payload = body_json(response).await;
        assert_eq!(payload["error"]["code"], "publication_reset_failed");
        assert_eq!(payload["error"]["details"]["intent_id"], intent_id);
    }
}

#[tokio::test]
async fn test_publication_recovery_client_reads_blocked_intents_over_http() {
    let base_url = serve_router(router_with_factory_query(FakeFactoryRunQuery {
        blocked_publications: Some(vec![sample_blocked_publication()]),
        ..Default::default()
    }))
    .await;

    let blocked = symphony::publication_recovery::fetch_blocked_publications(&base_url)
        .await
        .expect("blocked intents should be served");

    assert_eq!(blocked, vec![sample_blocked_publication()]);
}

#[tokio::test]
async fn test_publication_recovery_client_reads_an_empty_blocked_set() {
    let base_url = serve_router(router_with_factory_query(FakeFactoryRunQuery {
        blocked_publications: Some(Vec::new()),
        ..Default::default()
    }))
    .await;

    let blocked = symphony::publication_recovery::fetch_blocked_publications(&base_url)
        .await
        .expect("an empty set is a successful answer");

    assert!(blocked.is_empty());
    assert_eq!(
        symphony::publication_recovery::format_blocked_publications(&blocked),
        "no blocked publication intents"
    );
}

#[tokio::test]
async fn test_publication_recovery_client_resets_over_http() {
    let operators = Arc::new(std::sync::Mutex::new(Vec::new()));
    let base_url = serve_router(router_with_factory_query(FakeFactoryRunQuery {
        resets: BTreeMap::from([(
            "intent-blocked-1".to_string(),
            Ok(sample_publication_reset()),
        )]),
        reset_operators: Arc::clone(&operators),
        ..Default::default()
    }))
    .await;

    let reset = symphony::publication_recovery::reset_blocked_publication(
        &base_url,
        "intent-blocked-1",
        "ada",
    )
    .await
    .expect("reset should be served");

    assert_eq!(reset, sample_publication_reset());
    // The operator reaches the store as the audit event's attribution.
    assert_eq!(
        operators.lock().expect("operators lock").as_slice(),
        [("intent-blocked-1".to_string(), "ada".to_string())]
    );
}

#[tokio::test]
async fn test_publication_recovery_client_surfaces_a_refused_reset_without_falling_back() {
    // A reachable orchestrator that refuses the reset is an answer, not an
    // outage: falling back to the store here would only produce a lock error.
    let base_url = serve_router(router_with_factory_query(FakeFactoryRunQuery {
        resets: BTreeMap::from([(
            "intent-blocked-1".to_string(),
            Err("intent intent-blocked-1 is conflict, not blocked".to_string()),
        )]),
        ..Default::default()
    }))
    .await;

    let error = symphony::publication_recovery::reset_blocked_publication(
        &base_url,
        "intent-blocked-1",
        "ada",
    )
    .await
    .expect_err("a refused reset should surface");

    assert!(!error.is_unreachable(), "{error}");
    assert!(error.message().contains("not blocked"), "{error}");
    assert!(error.message().contains("409"), "{error}");
}

#[tokio::test]
async fn test_publication_recovery_client_falls_back_when_nothing_is_listening() {
    // This is the fallback trigger: Symphony is not running, so no lock is held
    // and the CLI can open the durable store directly.
    let base_url = unused_base_url().await;

    let list_error = symphony::publication_recovery::fetch_blocked_publications(&base_url)
        .await
        .expect_err("nothing is listening");
    assert!(list_error.is_unreachable(), "{list_error}");
    assert!(list_error.message().contains(&base_url), "{list_error}");

    let reset_error =
        symphony::publication_recovery::reset_blocked_publication(&base_url, "intent-1", "ada")
            .await
            .expect_err("nothing is listening");
    assert!(reset_error.is_unreachable(), "{reset_error}");

    // And the fallback's own failure explains the lock rather than leaking it.
    let message = symphony::publication_recovery::store_unavailable_message(
        &reset_error,
        "could not acquire exclusive triage store lock /data/factory.sqlite3.lock: locked",
    );
    assert!(message.contains("server.host and server.port"), "{message}");
}
