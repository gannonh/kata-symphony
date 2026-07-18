use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Method, Request, StatusCode};
use chrono::Utc;
use serde_json::{json, Value};
use symphony::domain::{
    CodexTotals, EscalationRequest, EscalationResponse, OrchestratorSnapshot, PollingSnapshot,
    RefreshRequestOutcome, SharedContextSummary,
};
use symphony::http_server::{build_router, HttpServerState, RefreshControl, SnapshotSource};
use symphony::orchestrator::{EscalationRegistry, EscalationResolveResult};
use tower::ServiceExt;

#[derive(Clone)]
struct StaticSnapshot {
    snapshot: OrchestratorSnapshot,
}

impl SnapshotSource for StaticSnapshot {
    fn snapshot(&self) -> OrchestratorSnapshot {
        self.snapshot.clone()
    }
}

#[derive(Default)]
struct NoopRefresh;

impl RefreshControl for NoopRefresh {
    fn request_refresh(&self) -> RefreshRequestOutcome {
        RefreshRequestOutcome {
            queued: false,
            coalesced: true,
            pending_requests: 0,
        }
    }
}

fn empty_snapshot() -> OrchestratorSnapshot {
    OrchestratorSnapshot {
        poll_interval_ms: 30_000,
        max_concurrent_agents: 4,
        tracker_project_url: None,
        running: BTreeMap::new(),
        running_sessions: BTreeMap::new(),
        running_session_info: BTreeMap::new(),
        claimed: BTreeSet::new(),
        retry_queue: vec![],
        completed: vec![],
        blocked: vec![],
        pending_escalations: vec![],
        shared_context: SharedContextSummary::default(),
        supervisor: symphony::domain::SupervisorSnapshot::default(),
        codex_totals: CodexTotals {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            event_count: 0,
            seconds_running: 0.0,
        },
        codex_rate_limits: None,
        polling: PollingSnapshot {
            checking: false,
            next_poll_in_ms: 0,
            poll_interval_ms: 30_000,
            last_poll_at: None,
            poll_count: 0,
        },
        triage_sessions: vec![],
    }
}

fn make_request(id: &str, issue_id: &str, issue_identifier: &str) -> EscalationRequest {
    EscalationRequest {
        id: id.to_string(),
        issue_id: issue_id.to_string(),
        issue_identifier: issue_identifier.to_string(),
        method: "ask_user_questions".to_string(),
        payload: json!({
            "questions": [
                {
                    "id": "q1",
                    "header": "Choice",
                    "question": "Pick one option"
                }
            ]
        }),
        created_at: Utc::now(),
        timeout_ms: 30_000,
    }
}

async fn read_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    serde_json::from_slice(&bytes).expect("response body should be valid json")
}

#[tokio::test]
async fn test_escalation_full_round_trip() {
    let registry = EscalationRegistry::default();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let request = make_request("esc-1", "issue-1", "KAT-1");

    registry.register(request.clone(), tx);
    assert_eq!(registry.pending_snapshot().len(), 1);

    let response = EscalationResponse {
        request_id: request.id.clone(),
        response: json!({"confirmed": true}),
        responder_id: Some("operator-1".to_string()),
        responded_at: Utc::now(),
    };

    let resolved = registry.resolve(&request.id, response.clone());
    assert!(matches!(resolved, EscalationResolveResult::Resolved));

    let delivered = rx.await.expect("worker receiver should get response");
    assert_eq!(delivered.request_id, request.id);
    assert_eq!(delivered.response, json!({"confirmed": true}));
    assert!(registry.pending_snapshot().is_empty());
}

#[tokio::test]
async fn test_escalation_timeout_cancels_cleanly() {
    let registry = EscalationRegistry::default();
    let (tx, rx) = tokio::sync::oneshot::channel::<EscalationResponse>();
    let request = make_request("esc-timeout", "issue-timeout", "KAT-2");

    registry.register(request, tx);
    let cancelled = registry.cancel_for_issue("issue-timeout");
    assert_eq!(cancelled.len(), 1);

    assert!(
        rx.await.is_err(),
        "dropping registry sender should cancel pending worker receive"
    );
}

#[tokio::test]
async fn test_escalation_cancel_for_issue_cleans_up() {
    let registry = EscalationRegistry::default();

    let (tx_a1, rx_a1) = tokio::sync::oneshot::channel::<EscalationResponse>();
    let (tx_a2, rx_a2) = tokio::sync::oneshot::channel::<EscalationResponse>();
    let (tx_b1, _rx_b1) = tokio::sync::oneshot::channel::<EscalationResponse>();

    registry.register(make_request("esc-a1", "issue-a", "KAT-A"), tx_a1);
    registry.register(make_request("esc-a2", "issue-a", "KAT-A"), tx_a2);
    registry.register(make_request("esc-b1", "issue-b", "KAT-B"), tx_b1);

    let cancelled = registry.cancel_for_issue("issue-a");
    assert_eq!(cancelled.len(), 2);

    let pending = registry.pending_snapshot();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].request_id, "esc-b1");

    assert!(rx_a1.await.is_err());
    assert!(rx_a2.await.is_err());
}

#[tokio::test]
async fn test_escalation_http_respond_endpoint() {
    let registry = EscalationRegistry::default();
    let request = make_request("esc-http", "issue-http", "KAT-HTTP");
    let (tx, rx) = tokio::sync::oneshot::channel::<EscalationResponse>();
    registry.register(request.clone(), tx);

    let state = HttpServerState::new(
        Arc::new(StaticSnapshot {
            snapshot: empty_snapshot(),
        }),
        Arc::new(NoopRefresh),
        registry.clone(),
    );
    let app = build_router(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/escalations/{}/respond", request.id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "response": {"confirmed": true, "value": "A"},
                        "responder_id": "operator-1"
                    })
                    .to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    assert_eq!(payload, json!({"ok": true}));

    let delivered = rx.await.expect("worker should receive escalation response");
    assert_eq!(delivered.request_id, request.id);
    assert_eq!(delivered.responder_id.as_deref(), Some("operator-1"));

    let second = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/escalations/{}/respond", request.id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"response": {"confirmed": false}}).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(second.status(), StatusCode::CONFLICT);
    let second_payload = read_json(second).await;
    assert_eq!(
        second_payload,
        json!({"error": "escalation_already_resolved"})
    );
}

#[tokio::test]
async fn test_escalation_http_respond_unknown_id() {
    let state = HttpServerState::new(
        Arc::new(StaticSnapshot {
            snapshot: empty_snapshot(),
        }),
        Arc::new(NoopRefresh),
        EscalationRegistry::default(),
    );
    let app = build_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v1/escalations/does-not-exist/respond")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"response": {"confirmed": true}}).to_string(),
                ))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let payload = read_json(response).await;
    assert_eq!(payload, json!({"error": "escalation_not_found"}));
}

#[tokio::test]
async fn test_escalation_http_list_pending() {
    let registry = EscalationRegistry::default();
    let (tx, _rx) = tokio::sync::oneshot::channel::<EscalationResponse>();
    registry.register(make_request("esc-list", "issue-list", "KAT-LIST"), tx);

    let state = HttpServerState::new(
        Arc::new(StaticSnapshot {
            snapshot: empty_snapshot(),
        }),
        Arc::new(NoopRefresh),
        registry,
    );
    let app = build_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/v1/escalations")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = read_json(response).await;
    let pending = payload["pending"]
        .as_array()
        .expect("pending should be array");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0]["request_id"], "esc-list");
}
