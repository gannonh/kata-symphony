use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{close_code, CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{Method, StatusCode, Uri};
use axum::response::{Html, IntoResponse};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::domain::{
    CodexTotals, ContextEntry, ContextScope, EventFilter, EventKind, EventSeverity,
    OrchestratorSnapshot, PendingEscalation, PollingSnapshot, RefreshRequestOutcome,
    RetrySnapshotEntry, RunAttempt, RunningSessionSnapshot, SharedContextSummary,
    SupervisorSnapshot, SymphonyEventEnvelope, WorkerSessionInfo,
};
use crate::event_stream::EventHub;
use crate::orchestrator::{
    EscalationRegistry, EscalationResolveResult, RefreshSender, SnapshotHandle, SteerResult,
    SteerSender,
};
use crate::shared_context::{
    ContextEntryDraft, SharedContextStore, MAX_SHARED_CONTEXT_CONTENT_CHARS,
};

pub const HTTP_PORT_RETRY_LIMIT: u16 = 10;

// ── Traits for testability ─────────────────────────────────────────────

pub trait SnapshotSource: Send + Sync {
    fn snapshot(&self) -> OrchestratorSnapshot;
}

impl<F> SnapshotSource for F
where
    F: Fn() -> OrchestratorSnapshot + Send + Sync,
{
    fn snapshot(&self) -> OrchestratorSnapshot {
        (self)()
    }
}

pub trait RefreshControl: Send + Sync {
    fn request_refresh(&self) -> RefreshRequestOutcome;
}

impl<F> RefreshControl for F
where
    F: Fn() -> RefreshRequestOutcome + Send + Sync,
{
    fn request_refresh(&self) -> RefreshRequestOutcome {
        (self)()
    }
}

/// Read-only factory-run queries for the A1 HTTP surface.
pub trait FactoryRunQuery: Send + Sync {
    fn get_run(&self, run_id: &str) -> Result<Option<FactoryRunHttpResponse>, String>;
    fn get_run_by_issue(
        &self,
        issue_identifier: &str,
    ) -> Result<Option<FactoryRunHttpResponse>, String>;
    fn triage_metrics(&self) -> Result<FactoryRunMetricsHttpResponse, String>;
    fn spec_metrics(&self) -> Result<FactoryRunMetricsHttpResponse, String> {
        Err("spec metrics are not available".to_string())
    }
    fn get_artifact(
        &self,
        _run_id: &str,
        _artifact_id: &str,
    ) -> Result<Option<FactoryArtifactHttpResponse>, String> {
        Ok(None)
    }
}

// ── Trait implementations for orchestrator types ───────────────────────

impl SnapshotSource for SnapshotHandle {
    fn snapshot(&self) -> OrchestratorSnapshot {
        self.read()
    }
}

impl RefreshControl for RefreshSender {
    fn request_refresh(&self) -> RefreshRequestOutcome {
        self.request_refresh()
    }
}

// ── Event hub + HTTP server state ──────────────────────────────────────

const DEFAULT_WS_HEARTBEAT_INTERVAL_MS: u64 = 5_000;
const DEFAULT_WS_CLIENT_QUEUE_CAPACITY: usize = 64;
const DEFAULT_WS_BACKPRESSURE_DROP_THRESHOLD: u64 = 1;
const WS_CLOSE_ENQUEUE_TIMEOUT_MS: u64 = 1_000;
const MAX_STEER_INSTRUCTION_CHARS: usize = 5_000;

#[derive(Debug, Clone)]
pub struct EventStreamConfig {
    pub heartbeat_interval: Duration,
    pub client_queue_capacity: usize,
    pub backpressure_drop_threshold: u64,
    /// Test-only seam to make slow-consumer behavior deterministic.
    pub writer_send_delay: Option<Duration>,
}

impl Default for EventStreamConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_millis(DEFAULT_WS_HEARTBEAT_INTERVAL_MS),
            client_queue_capacity: DEFAULT_WS_CLIENT_QUEUE_CAPACITY,
            backpressure_drop_threshold: DEFAULT_WS_BACKPRESSURE_DROP_THRESHOLD,
            writer_send_delay: None,
        }
    }
}

#[derive(Default)]
struct EventStreamCounters {
    connected: AtomicU64,
    disconnected: AtomicU64,
    dropped: AtomicU64,
    heartbeat: AtomicU64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct EventStreamCounterSnapshot {
    pub connected: u64,
    pub disconnected: u64,
    pub dropped: u64,
    pub heartbeat: u64,
}

#[derive(Clone)]
pub struct HttpServerState {
    snapshot_source: Arc<dyn SnapshotSource>,
    refresh_control: Arc<dyn RefreshControl>,
    escalation_registry: EscalationRegistry,
    event_hub: EventHub,
    shared_context_store: SharedContextStore,
    steer_sender: Option<SteerSender>,
    factory_run_query: Option<Arc<dyn FactoryRunQuery>>,
    event_stream_config: EventStreamConfig,
    event_stream_counters: Arc<EventStreamCounters>,
    next_client_id: Arc<AtomicU64>,
}

impl HttpServerState {
    pub fn new(
        snapshot_source: Arc<dyn SnapshotSource>,
        refresh_control: Arc<dyn RefreshControl>,
        escalation_registry: EscalationRegistry,
    ) -> Self {
        Self::with_event_stream(
            snapshot_source,
            refresh_control,
            escalation_registry,
            EventHub::default_hub(),
            EventStreamConfig::default(),
        )
    }

    pub fn with_event_stream(
        snapshot_source: Arc<dyn SnapshotSource>,
        refresh_control: Arc<dyn RefreshControl>,
        escalation_registry: EscalationRegistry,
        event_hub: EventHub,
        event_stream_config: EventStreamConfig,
    ) -> Self {
        Self {
            snapshot_source,
            refresh_control,
            escalation_registry,
            event_hub,
            shared_context_store: SharedContextStore::default(),
            steer_sender: None,
            factory_run_query: None,
            event_stream_config,
            event_stream_counters: Arc::new(EventStreamCounters::default()),
            next_client_id: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn with_shared_context_store(mut self, shared_context_store: SharedContextStore) -> Self {
        self.shared_context_store = shared_context_store;
        self
    }

    pub fn with_steer_sender(mut self, steer_sender: SteerSender) -> Self {
        self.steer_sender = Some(steer_sender);
        self
    }

    pub fn with_factory_run_query(mut self, factory_run_query: Arc<dyn FactoryRunQuery>) -> Self {
        self.factory_run_query = Some(factory_run_query);
        self
    }

    pub fn snapshot(&self) -> OrchestratorSnapshot {
        self.snapshot_source.snapshot()
    }

    pub fn request_refresh(&self) -> RefreshRequestOutcome {
        self.refresh_control.request_refresh()
    }

    pub fn resolve_escalation(
        &self,
        request_id: &str,
        response: serde_json::Value,
        responder_id: Option<String>,
    ) -> EscalationResolveResult {
        self.escalation_registry.resolve(
            request_id,
            crate::domain::EscalationResponse {
                request_id: request_id.to_string(),
                response,
                responder_id,
                responded_at: Utc::now(),
            },
        )
    }

    pub fn pending_escalations(&self) -> Vec<PendingEscalation> {
        self.escalation_registry.pending_snapshot()
    }

    pub fn event_hub(&self) -> EventHub {
        self.event_hub.clone()
    }

    pub fn shared_context_store(&self) -> SharedContextStore {
        self.shared_context_store.clone()
    }

    pub async fn request_steer(
        &self,
        issue_identifier: String,
        instruction: String,
    ) -> SteerResult {
        let Some(steer_sender) = &self.steer_sender else {
            return SteerResult::DeliveryFailed {
                message: "steer_unavailable".to_string(),
            };
        };

        steer_sender
            .request_steer(issue_identifier, instruction, Duration::from_secs(5))
            .await
    }

    pub fn event_stream_config(&self) -> EventStreamConfig {
        self.event_stream_config.clone()
    }

    pub fn next_client_id(&self) -> u64 {
        self.next_client_id.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn increment_connected(&self) -> u64 {
        self.event_stream_counters
            .connected
            .fetch_add(1, Ordering::SeqCst)
            + 1
    }

    pub fn increment_disconnected(&self) -> u64 {
        self.event_stream_counters
            .disconnected
            .fetch_add(1, Ordering::SeqCst)
            + 1
    }

    pub fn increment_dropped(&self, delta: u64) -> u64 {
        self.event_stream_counters
            .dropped
            .fetch_add(delta, Ordering::SeqCst)
            + delta
    }

    pub fn increment_heartbeat(&self) -> u64 {
        self.event_stream_counters
            .heartbeat
            .fetch_add(1, Ordering::SeqCst)
            + 1
    }

    pub fn event_stream_counters(&self) -> EventStreamCounterSnapshot {
        EventStreamCounterSnapshot {
            connected: self.event_stream_counters.connected.load(Ordering::SeqCst),
            disconnected: self
                .event_stream_counters
                .disconnected
                .load(Ordering::SeqCst),
            dropped: self.event_stream_counters.dropped.load(Ordering::SeqCst),
            heartbeat: self.event_stream_counters.heartbeat.load(Ordering::SeqCst),
        }
    }
}

// ── API Response Types ─────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ApiErrorEnvelope {
    error: ApiError,
}

#[derive(Debug, Serialize)]
struct ApiError {
    code: &'static str,
    message: String,
    status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct StateResponse {
    poll_interval_ms: u64,
    max_concurrent_agents: u32,
    tracker_project_url: Option<String>,
    running: std::collections::BTreeMap<String, RunAttempt>,
    running_sessions: std::collections::BTreeMap<String, RunningSessionSnapshot>,
    running_session_info: std::collections::BTreeMap<String, WorkerSessionInfo>,
    claimed: std::collections::BTreeSet<String>,
    retry_queue: Vec<RetrySnapshotEntry>,
    blocked: Vec<crate::domain::BlockedIssueEntry>,
    pending_escalations: Vec<PendingEscalation>,
    completed: Vec<crate::domain::CompletedEntry>,
    shared_context: SharedContextSummary,
    supervisor: SupervisorSnapshot,
    codex_totals: CodexTotals,
    codex_rate_limits: Option<serde_json::Value>,
    polling: PollingSnapshot,
}

#[derive(Debug, Serialize)]
struct IssueResponseEnvelope {
    issue: IssueProjection,
}

#[derive(Debug, Serialize)]
struct IssueProjection {
    issue_id: String,
    issue_identifier: String,
    status: &'static str,
    attempt: Option<u32>,
    error: Option<String>,
    worker_host: Option<String>,
    workspace_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EscalationRespondRequest {
    response: serde_json::Value,
    #[serde(default)]
    responder_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SteerRequestBody {
    issue_identifier: Option<String>,
    instruction: Option<String>,
}

#[derive(Debug, Serialize)]
struct SteerResponseBody {
    ok: bool,
    issue_id: String,
    issue_identifier: String,
    delivered: bool,
    instruction_preview: String,
}

#[derive(Debug, Serialize)]
struct EscalationPendingResponse {
    pending: Vec<PendingEscalation>,
}

#[derive(Debug, Serialize)]
struct RefreshResponse {
    queued: bool,
    coalesced: bool,
    pending_requests: u64,
}

#[derive(Debug, Deserialize)]
struct SharedContextWriteRequest {
    author_issue: String,
    scope: String,
    content: String,
    ttl_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct SharedContextQuery {
    scope: Option<String>,
}

#[derive(Debug, Serialize)]
struct SharedContextListResponse {
    entries: Vec<ContextEntry>,
    summary: SharedContextSummary,
}

#[derive(Debug, Serialize)]
struct SharedContextWriteResponse {
    id: String,
    created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct SharedContextDeleteResponse {
    deleted: usize,
}

#[derive(Debug, Deserialize, Default)]
struct EventFilterQuery {
    issue: Option<String>,
    #[serde(rename = "type")]
    event_type: Option<String>,
    severity: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct FactoryRunsListQuery {
    issue: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct FactoryRunsMetricsQuery {
    stage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactoryRunHttpResponse {
    pub run_id: String,
    pub forge_host: String,
    pub repository: String,
    pub issue: FactoryRunIssueHttp,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_stage: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
    pub attempts: Vec<FactoryRunAttemptHttp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<FactoryRunArtifactHttp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication: Option<FactoryRunPublicationHttp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<FactoryRunSpecHttp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryRunIssueHttp {
    pub id: String,
    pub identifier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryRunAttemptHttp {
    pub stage_run_id: String,
    #[serde(default = "default_triage_stage")]
    pub stage: String,
    pub attempt: u32,
    pub status: String,
    pub configuration_revision: String,
    pub harness: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<chrono::DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<chrono::DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub usage: crate::triage::domain::StageUsage,
    #[serde(default)]
    pub error: Option<crate::triage::domain::FactoryError>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub turns: Vec<FactoryRunSpecTurnHttp>,
}

fn default_triage_stage() -> String {
    crate::triage::domain::TRIAGE_STAGE_NAME.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryRunSpecTurnHttp {
    pub turn_id: String,
    pub ordinal: u32,
    pub kind: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub started_at: chrono::DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<chrono::DateTime<Utc>>,
    pub usage: crate::triage::domain::StageUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::triage::domain::FactoryError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryRunSpecVersionHttp {
    pub artifact_id: String,
    pub version: u32,
    pub attempt: u32,
    pub review_cycles: u32,
    pub unresolved_blocking_findings: usize,
    pub published: bool,
    pub received_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryRunSpecHttp {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_approval_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_version: Option<u32>,
    pub versions: Vec<FactoryRunSpecVersionHttp>,
    pub revision_requests_used: u32,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication: Option<FactoryRunSpecPublicationHttp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryRunSpecPublicationHttp {
    pub intent_id: String,
    pub kind: String,
    pub status: String,
    pub completed_steps: Vec<String>,
    pub retry_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::triage::domain::FactoryError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactoryArtifactHttpResponse {
    pub artifact_id: String,
    pub run_id: String,
    pub stage_run_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    pub attempt: u32,
    pub received_at: chrono::DateTime<Utc>,
    pub artifact: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryRunArtifactHttp {
    pub artifact_id: String,
    pub schema_version: u32,
    pub route: String,
    pub risk_class: String,
    pub rationale: String,
    pub evidence: Vec<crate::triage::domain::TriageEvidence>,
    pub next_action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clarification_question: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reproduction: Option<crate::triage::domain::ReproductionSummary>,
    pub received_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactoryRunPublicationHttp {
    pub intent_id: String,
    pub mode: String,
    pub status: String,
    pub completed_steps: Vec<String>,
    pub route_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_state: Option<String>,
    pub retry_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<crate::triage::domain::FactoryError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactoryRunMetricsHttpResponse {
    pub stage: String,
    pub total_attempts: u64,
    pub completed_attempts: u64,
    pub failed_attempts: u64,
    pub ineligible_issues: u64,
    pub route_counts: std::collections::BTreeMap<String, u64>,
    pub correction_count: u64,
    pub correction_rate: f64,
    pub duration: crate::triage::domain::TriageMetricsDuration,
    pub tokens_by_harness_model:
        std::collections::BTreeMap<String, crate::triage::domain::TriageMetricsTokenTotals>,
}

/// Build the HTTP factory-run response from durable store records.
pub fn factory_run_http_response(
    run: &crate::triage::domain::FactoryRunRecord,
    attempts: &[crate::triage::domain::StageRunRecord],
    artifact: Option<&crate::triage::domain::ArtifactRecord>,
    publication: Option<&crate::triage::domain::PublicationIntentRecord>,
) -> FactoryRunHttpResponse {
    FactoryRunHttpResponse {
        run_id: run.run_id.clone(),
        forge_host: run.forge_host.clone(),
        repository: run.repository.clone(),
        issue: FactoryRunIssueHttp {
            id: run.issue_id.clone(),
            identifier: run.issue_identifier.clone(),
            revision: run.issue_revision.clone(),
        },
        status: run.status.as_str().to_string(),
        current_stage: run.current_stage.clone(),
        created_at: run.created_at,
        updated_at: run.updated_at,
        attempts: attempts
            .iter()
            .map(|attempt| {
                let duration_ms = match (attempt.started_at, attempt.completed_at) {
                    (Some(start), Some(end)) => {
                        Some((end - start).num_milliseconds().max(0) as u64)
                    }
                    _ => None,
                };
                FactoryRunAttemptHttp {
                    stage_run_id: attempt.stage_run_id.clone(),
                    stage: attempt.stage.clone(),
                    attempt: attempt.attempt,
                    status: attempt.status.as_str().to_string(),
                    configuration_revision: attempt.configuration_revision.clone(),
                    harness: attempt.harness.clone(),
                    model: attempt.model.clone(),
                    started_at: attempt.started_at,
                    completed_at: attempt.completed_at,
                    duration_ms,
                    usage: attempt.usage.clone(),
                    error: attempt.error.clone(),
                    turns: Vec::new(),
                }
            })
            .collect(),
        artifact: artifact.map(|record| FactoryRunArtifactHttp {
            artifact_id: record.artifact_id.clone(),
            schema_version: record.artifact.schema_version,
            route: record.artifact.route.as_str().to_string(),
            risk_class: record.artifact.risk_class.as_str().to_string(),
            rationale: record.artifact.rationale.clone(),
            evidence: record.artifact.evidence.clone(),
            next_action: record.artifact.next_action.clone(),
            clarification_question: record.artifact.clarification_question.clone(),
            reproduction: record.artifact.reproduction.clone(),
            received_at: record.received_at,
        }),
        publication: publication.map(|intent| FactoryRunPublicationHttp {
            intent_id: intent.intent_id.clone(),
            mode: intent.mode.as_str().to_string(),
            status: intent.status.as_str().to_string(),
            completed_steps: intent.completed_steps.clone(),
            route_label: intent.route_label.clone(),
            project_state: intent.project_state.clone(),
            retry_count: intent.retry_count,
            error: intent.last_error.clone(),
        }),
        spec: None,
    }
}

pub fn attach_spec_http_response(
    response: &mut FactoryRunHttpResponse,
    artifacts: &[crate::spec::domain::SpecArtifactRecord],
    state: Option<&crate::spec::domain::SpecRunState>,
    publication: Option<&crate::spec::domain::SpecPublicationIntent>,
    turns: &std::collections::HashMap<String, Vec<crate::spec::domain::SpecTurnRecord>>,
) {
    if artifacts.is_empty() && state.is_none() && publication.is_none() && turns.is_empty() {
        return;
    }
    for attempt in &mut response.attempts {
        if let Some(records) = turns.get(&attempt.stage_run_id) {
            attempt.turns = records
                .iter()
                .map(|turn| FactoryRunSpecTurnHttp {
                    turn_id: turn.turn_id.clone(),
                    ordinal: turn.ordinal,
                    kind: turn.kind.as_str().to_string(),
                    status: turn.status.as_str().to_string(),
                    model: turn.model.clone(),
                    started_at: turn.started_at,
                    completed_at: turn.completed_at,
                    usage: turn.usage.clone(),
                    error: turn.error.clone(),
                })
                .collect();
        }
    }
    let attempt_by_stage: std::collections::HashMap<&str, u32> = response
        .attempts
        .iter()
        .map(|attempt| (attempt.stage_run_id.as_str(), attempt.attempt))
        .collect();
    let current_version = artifacts.iter().map(|artifact| artifact.version).max();
    response.spec = Some(FactoryRunSpecHttp {
        current_version,
        pending_approval_version: state.and_then(|state| state.pending_approval_version),
        approved_version: state.and_then(|state| state.approved_version),
        versions: artifacts
            .iter()
            .map(|artifact| FactoryRunSpecVersionHttp {
                artifact_id: artifact.artifact_id.clone(),
                version: artifact.version,
                attempt: attempt_by_stage
                    .get(artifact.stage_run_id.as_str())
                    .copied()
                    .unwrap_or_default(),
                review_cycles: artifact.review_cycles,
                unresolved_blocking_findings: artifact.unresolved_blocking_findings.len(),
                published: true,
                received_at: artifact.received_at,
            })
            .collect(),
        revision_requests_used: state
            .map(|state| state.revision_requests_used)
            .unwrap_or_default(),
        decision: state
            .map(|state| state.decision.as_str().to_string())
            .unwrap_or_else(|| "awaiting_decision".to_string()),
        publication: publication.map(|intent| FactoryRunSpecPublicationHttp {
            intent_id: intent.intent_id.clone(),
            kind: intent.kind.as_str().to_string(),
            status: intent.status.as_str().to_string(),
            completed_steps: intent.completed_steps.clone(),
            retry_count: intent.retry_count,
            error: intent.last_error.clone(),
        }),
    });
}

pub fn factory_run_metrics_http_response(
    metrics: crate::triage::domain::TriageMetricsAggregate,
) -> FactoryRunMetricsHttpResponse {
    FactoryRunMetricsHttpResponse {
        stage: crate::triage::domain::TRIAGE_STAGE_NAME.to_string(),
        total_attempts: metrics.total_attempts,
        completed_attempts: metrics.completed_attempts,
        failed_attempts: metrics.failed_attempts,
        ineligible_issues: metrics.ineligible_issues,
        route_counts: metrics.route_counts,
        correction_count: metrics.correction_count,
        correction_rate: metrics.correction_rate,
        duration: metrics.duration,
        tokens_by_harness_model: metrics.tokens_by_harness_model,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventFilterError {
    pub field: &'static str,
    pub value: String,
    pub message: String,
}

impl EventFilterError {
    fn to_api_error(&self) -> ApiErrorEnvelope {
        ApiErrorEnvelope {
            error: ApiError {
                code: "invalid_filter",
                message: self.message.clone(),
                status: StatusCode::BAD_REQUEST.as_u16(),
                details: Some(serde_json::json!({
                    "field": self.field,
                    "value": self.value,
                })),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WsCloseReason {
    ClientClosed,
    Backpressure,
    ServerShutdown,
    ProtocolError,
}

impl WsCloseReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::ClientClosed => "client_closed",
            Self::Backpressure => "backpressure",
            Self::ServerShutdown => "server_shutdown",
            Self::ProtocolError => "protocol_error",
        }
    }

    fn close_frame(self) -> CloseFrame {
        match self {
            Self::ClientClosed => CloseFrame {
                code: close_code::NORMAL,
                reason: "client_closed".into(),
            },
            Self::Backpressure => CloseFrame {
                code: close_code::POLICY,
                reason: "backpressure".into(),
            },
            Self::ServerShutdown => CloseFrame {
                code: close_code::RESTART,
                reason: "server_shutdown".into(),
            },
            Self::ProtocolError => CloseFrame {
                code: close_code::PROTOCOL,
                reason: "protocol_error".into(),
            },
        }
    }
}

enum OutboundMessage {
    Envelope(SymphonyEventEnvelope),
    Pong(Vec<u8>),
    Close(WsCloseReason),
}

// ── Router ─────────────────────────────────────────────────────────────

pub fn build_router(state: HttpServerState) -> Router {
    Router::new()
        .route("/", get(get_dashboard))
        .route("/api/v1/state", get(get_state))
        .route(
            "/api/v1/context",
            get(get_context).post(post_context).delete(delete_context),
        )
        .route("/api/v1/context/{entry_id}", delete(delete_context_entry))
        .route("/api/v1/events", get(get_events))
        .route("/api/v1/escalations", get(get_escalations))
        .route(
            "/api/v1/escalations/{request_id}/respond",
            post(post_escalation_respond),
        )
        .route("/api/v1/steer", post(post_steer))
        .route("/api/v1/factory-runs/metrics", get(get_factory_run_metrics))
        .route(
            "/api/v1/factory-runs/{run_id}/artifacts/{artifact_id}",
            get(get_factory_artifact),
        )
        .route("/api/v1/factory-runs/{run_id}", get(get_factory_run_by_id))
        .route("/api/v1/factory-runs", get(get_factory_runs))
        .route("/api/v1/{issue_identifier}", get(get_issue))
        .route("/api/v1/refresh", post(post_refresh))
        .fallback(api_not_found)
        .method_not_allowed_fallback(api_method_not_allowed)
        .with_state(state)
}

pub async fn start_http_server(
    state: HttpServerState,
    listener: TcpListener,
    host: &str,
    configured_port: u16,
    bound_port: u16,
) -> std::io::Result<()> {
    tracing::info!(
        event = "http_server_started",
        host = host,
        configured_port,
        port = bound_port,
        "HTTP observability server started"
    );
    axum::serve(listener, build_router(state)).await
}

pub async fn bind_http_listener_with_fallback(
    host: &str,
    configured_port: u16,
    max_port_offset: u16,
) -> std::io::Result<(TcpListener, u16)> {
    if configured_port == 0 {
        let listener = TcpListener::bind(format!("{host}:0")).await?;
        let bound_port = listener.local_addr()?.port();
        return Ok((listener, bound_port));
    }

    let mut attempt_port = configured_port;
    let max_port = configured_port.saturating_add(max_port_offset);

    loop {
        let bind_addr = format!("{host}:{attempt_port}");
        match TcpListener::bind(&bind_addr).await {
            Ok(listener) => {
                let bound_port = listener.local_addr()?.port();
                if attempt_port != configured_port {
                    tracing::warn!(
                        event = "http_server_port_auto_incremented",
                        host = host,
                        configured_port,
                        bound_port,
                        retries = attempt_port.saturating_sub(configured_port),
                        "Configured HTTP port was in use; bound the next available port"
                    );
                }
                return Ok((listener, bound_port));
            }
            Err(err) if err.kind() == std::io::ErrorKind::AddrInUse && attempt_port < max_port => {
                tracing::warn!(
                    event = "http_server_port_in_use_retry",
                    host = host,
                    configured_port,
                    attempted_port = attempt_port,
                    next_port = attempt_port + 1,
                    max_port,
                    "Configured HTTP port is in use; retrying on next port"
                );
                attempt_port += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

// ── Route Handlers ─────────────────────────────────────────────────────

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn truncate_preview(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

async fn get_dashboard(State(state): State<HttpServerState>) -> impl IntoResponse {
    let snapshot = state.snapshot();

    let running_count = snapshot.running.len();
    let escalation_count = snapshot.pending_escalations.len();
    let retry_count = snapshot.retry_queue.len();
    let completed_count = snapshot.completed.len();
    let claimed_count = snapshot.claimed.len();
    let input_tokens = snapshot.codex_totals.input_tokens;
    let output_tokens = snapshot.codex_totals.output_tokens;
    let total_tokens = snapshot.codex_totals.total_tokens;
    let polling_checking = if snapshot.polling.checking {
        "yes"
    } else {
        "no"
    };
    let supervisor_status_label = match snapshot.supervisor.status {
        crate::domain::SupervisorStatus::Disabled => "⚪ disabled",
        crate::domain::SupervisorStatus::Starting => "🟡 starting",
        crate::domain::SupervisorStatus::Active => "🟢 active",
        crate::domain::SupervisorStatus::Stopped => "⚫ stopped",
        crate::domain::SupervisorStatus::Failed => "🔴 failed",
    };
    let supervisor_steers = snapshot.supervisor.steers_issued;
    let supervisor_conflicts = snapshot.supervisor.conflicts_detected;
    let supervisor_patterns = snapshot.supervisor.patterns_detected;
    let supervisor_escalations = snapshot.supervisor.escalations_created;
    let supervisor_last_decision = snapshot
        .supervisor
        .last_decision
        .as_deref()
        .map(escape_html)
        .unwrap_or_else(|| "n/a".to_string());
    let supervisor_last_action = snapshot
        .supervisor
        .last_action_at
        .map(|timestamp| timestamp.to_rfc3339())
        .map(|value| escape_html(&value))
        .unwrap_or_else(|| "n/a".to_string());
    let tracker_project_card = snapshot
        .tracker_project_url
        .as_deref()
        .map(|url| {
            let escaped_url = escape_html(url);
            format!(
                r#"<section class="card"><div class="label">project</div><div class="mono"><a id="tracker-project-link" href="{escaped_url}" target="_blank" rel="noopener noreferrer">{escaped_url}</a></div></section>"#
            )
        })
        .unwrap_or_else(|| {
            r#"<section class="card"><div class="label">project</div><div class="mono muted">n/a</div></section>"#
                .to_string()
        });

    let rate_limit_block = snapshot
        .codex_rate_limits
        .as_ref()
        .map(|rate_limits| {
            serde_json::to_string_pretty(&rate_limits.data).unwrap_or_else(|_| "{}".to_string())
        })
        .unwrap_or_else(|| "{}".to_string());

    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Symphony Dashboard</title>
  <style>
    :root {{ color-scheme: dark; }}
    body {{ font-family: -apple-system, BlinkMacSystemFont, Segoe UI, sans-serif; background: #10131a; color: #f1f5f9; margin: 0; padding: 24px; line-height: 1.45; }}
    a {{ color: #93c5fd; }}
    a:hover {{ color: #bfdbfe; }}
    h1 {{ margin-top: 0; margin-bottom: 12px; }}
    h2 {{ margin: 0 0 10px; font-size: 18px; }}
    .grid {{ display: grid; gap: 12px; margin-bottom: 16px; }}
    .summary-grid {{ grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); }}
    .token-grid {{ grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); margin-bottom: 0; }}
    .card {{ background: #1c2430; border: 1px solid #314154; border-radius: 10px; padding: 12px; }}
    .label {{ color: #94a3b8; font-size: 12px; text-transform: uppercase; letter-spacing: .03em; }}
    .value {{ font-size: 24px; font-weight: 700; margin-top: 4px; }}
    .mono {{ font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }}
    .section {{ margin-top: 12px; }}
    .table-wrap {{ overflow-x: auto; }}
    table {{ width: 100%; border-collapse: collapse; font-size: 14px; }}
    th, td {{ padding: 8px; border-bottom: 1px solid #2b3a4e; text-align: left; vertical-align: top; }}
    th {{ color: #a8b8cc; font-size: 12px; text-transform: uppercase; letter-spacing: .03em; }}
    td {{ color: #d7e1ee; }}
    td.mono {{ word-break: break-all; }}
    .stale-activity {{ color: #fca5a5; font-weight: 600; }}
    .error-text {{ color: #e5484d; font-weight: 600; }}
    .list {{ margin: 0; padding-left: 20px; }}
    .muted {{ color: #94a3b8; }}
    .error {{ margin-top: 12px; color: #fca5a5; }}
    .polling-grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 8px; margin: 0; }}
    .polling-grid div {{ background: #0f1724; border: 1px solid #27354a; border-radius: 8px; padding: 8px; }}
    details summary {{ cursor: pointer; font-weight: 600; }}
    pre {{ white-space: pre-wrap; word-break: break-word; background: #0b1119; border: 1px solid #27354a; border-radius: 8px; padding: 10px; margin-top: 10px; }}
  </style>
</head>
<body>
  <h1>Symphony Dashboard</h1>
  <div class="grid summary-grid">
    <section class="card"><div class="label">running</div><div class="value" id="running-count">{running_count}</div></section>
    <section class="card"><div class="label">escalations</div><div class="value" id="escalation-count">{escalation_count}</div></section>
    <section class="card"><div class="label">retry</div><div class="value" id="retry-count">{retry_count}</div></section>
    <section class="card"><div class="label">claimed</div><div class="value" id="claimed-count">{claimed_count}</div></section>
    <section class="card"><div class="label">completed</div><div class="value" id="completed-count">{completed_count}</div></section>
    <section class="card"><div class="label">supervisor</div><div class="mono" id="supervisor-status">{supervisor_status_label}</div></section>
    <div id="tracker-project-card">{tracker_project_card}</div>
  </div>

  <section class="card section">
    <h2>Token summary</h2>
    <div class="grid token-grid">
      <div><div class="label">input tokens</div><div class="value" id="token-input">{input_tokens}</div></div>
      <div><div class="label">output tokens</div><div class="value" id="token-output">{output_tokens}</div></div>
      <div><div class="label">total tokens</div><div class="value" id="token-total">{total_tokens}</div></div>
    </div>
  </section>

  <section class="card section">
    <h2>Running sessions</h2>
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>Identifier</th>
            <th>Tracker State</th>
            <th>Status</th>
            <th>Activity</th>
            <th>Error</th>
            <th>Attempt</th>
            <th>Turn</th>
            <th>Last Activity</th>
            <th>Elapsed</th>
            <th>Tokens</th>
            <th>Model</th>
            <th>Workspace</th>
            <th>Worker host</th>
          </tr>
        </thead>
        <tbody id="running-table-body">
          <tr><td class="muted" colspan="13">Loading...</td></tr>
        </tbody>
      </table>
    </div>
  </section>

  <section class="card section">
    <h2>Pending Escalations</h2>
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>Issue</th>
            <th>Method</th>
            <th>Question Preview</th>
            <th>Waiting</th>
            <th>Timeout</th>
          </tr>
        </thead>
        <tbody id="escalation-table-body">
          <tr><td class="muted" colspan="5">Loading...</td></tr>
        </tbody>
      </table>
    </div>
  </section>

  <section class="card section">
    <h2>Retry queue</h2>
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>Identifier</th>
            <th>Attempt</th>
            <th>Error</th>
            <th>Retry after</th>
            <th>Worker host</th>
          </tr>
        </thead>
        <tbody id="retry-table-body">
          <tr><td class="muted" colspan="5">Loading...</td></tr>
        </tbody>
      </table>
    </div>
  </section>

  <section class="card section" id="blocked-section" style="display:none">
    <h2 style="color:#e6a817">Blocked issues</h2>
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>ID</th>
            <th>Title</th>
            <th>State</th>
            <th>Blocked by</th>
          </tr>
        </thead>
        <tbody id="blocked-table-body"></tbody>
      </table>
    </div>
  </section>

  <section class="card section">
    <details id="shared-context-details" open>
      <summary>Shared Context (<span id="context-count">0</span>)</summary>
      <div class="table-wrap" style="margin-top: 10px;">
        <table>
          <thead>
            <tr>
              <th>Author</th>
              <th>Scope</th>
              <th>Content Preview</th>
              <th>Age</th>
              <th>TTL Remaining</th>
            </tr>
          </thead>
          <tbody id="context-table-body">
            <tr><td class="muted" colspan="5">Loading...</td></tr>
          </tbody>
        </table>
      </div>
    </details>
  </section>

  <section class="card section">
    <h2>Supervisor</h2>
    <div class="polling-grid">
      <div><div class="label">status</div><div class="mono" id="supervisor-status-detail">{supervisor_status_label}</div></div>
      <div><div class="label">steers</div><div class="mono" id="supervisor-steers">{supervisor_steers}</div></div>
      <div><div class="label">conflicts</div><div class="mono" id="supervisor-conflicts">{supervisor_conflicts}</div></div>
      <div><div class="label">patterns</div><div class="mono" id="supervisor-patterns">{supervisor_patterns}</div></div>
      <div><div class="label">escalations</div><div class="mono" id="supervisor-escalations">{supervisor_escalations}</div></div>
      <div><div class="label">last decision</div><div class="mono" id="supervisor-last-decision">{supervisor_last_decision}</div></div>
      <div><div class="label">last action</div><div class="mono" id="supervisor-last-action">{supervisor_last_action}</div></div>
    </div>
  </section>

  <section class="card section">
    <h2>Completed issues</h2>
    <ul id="completed-list" class="list">
      <li class="muted">Loading...</li>
    </ul>
  </section>

  <section class="card section">
    <h2>Polling</h2>
    <div class="polling-grid">
      <div><div class="label">last poll at</div><div class="mono" id="polling-last-poll">n/a</div></div>
      <div><div class="label">poll count</div><div class="mono" id="polling-count">n/a</div></div>
      <div><div class="label">checking</div><div class="mono" id="polling-checking">{polling_checking}</div></div>
      <div><div class="label">next poll in</div><div class="mono" id="polling-next-poll">n/a</div></div>
      <div><div class="label">poll interval</div><div class="mono" id="polling-interval">n/a</div></div>
    </div>
  </section>

  <details class="card section" open>
    <summary>Rate limits</summary>
    <pre id="rate-limits" class="mono">{rate_limit_block}</pre>
  </details>

  <p id="refresh-error" class="error" hidden></p>

  <script>
    const STALE_ACTIVITY_THRESHOLD_MS = 120000;

    function escapeHtml(value) {{
      return String(value)
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;');
    }}

    function formatDuration(ms) {{
      if (ms === null || ms === undefined) return 'n/a';
      const numeric = Number(ms);
      if (!Number.isFinite(numeric)) return 'n/a';
      const sign = numeric < 0 ? '-' : '';
      const totalSeconds = Math.floor(Math.abs(numeric) / 1000);
      const hours = Math.floor(totalSeconds / 3600);
      const minutes = Math.floor((totalSeconds % 3600) / 60);
      const seconds = totalSeconds % 60;
      if (hours > 0) return sign + hours + 'h ' + minutes + 'm ' + seconds + 's';
      if (minutes > 0) return sign + minutes + 'm ' + seconds + 's';
      return sign + seconds + 's';
    }}

    function formatTimeAgo(ms) {{
      const numeric = Number(ms);
      if (!Number.isFinite(numeric)) return 'n/a';
      if (numeric <= 0) return 'just now';
      const totalSeconds = Math.floor(numeric / 1000);
      if (totalSeconds < 60) return totalSeconds + 's ago';
      const totalMinutes = Math.floor(totalSeconds / 60);
      if (totalMinutes < 60) return totalMinutes + 'm ago';
      const totalHours = Math.floor(totalMinutes / 60);
      if (totalHours < 24) return totalHours + 'h ago';
      const totalDays = Math.floor(totalHours / 24);
      return totalDays + 'd ago';
    }}

    function formatRetryDelay(ms) {{
      const numeric = Number(ms);
      if (!Number.isFinite(numeric)) return 'n/a';
      if (numeric <= 0) return 'ready';
      return 'in ' + formatDuration(numeric);
    }}

    function formatDate(value) {{
      if (value === null || value === undefined) return 'n/a';
      if (typeof value === 'string') {{
        const parsed = new Date(value);
        return Number.isNaN(parsed.getTime()) ? value : parsed.toISOString();
      }}
      const numeric = Number(value);
      if (!Number.isFinite(numeric)) return String(value);
      const parsed = new Date(numeric);
      return Number.isNaN(parsed.getTime()) ? String(value) : parsed.toISOString();
    }}

    function issueNumberFromIdentifier(identifier) {{
      const match = String(identifier || '').match(/(\d+)/);
      return match ? match[1] : null;
    }}

    function buildIssueUrl(issueIdentifier, issueUrl, trackerProjectUrl) {{
      if (typeof issueUrl === 'string' && issueUrl.trim().length > 0) {{
        return issueUrl.trim();
      }}

      if (typeof trackerProjectUrl !== 'string' || trackerProjectUrl.trim().length === 0) {{
        return null;
      }}

      const projectBase = trackerProjectUrl.trim();
      if (!projectBase.includes('/issues')) {{
        return null;
      }}

      const issueNumber = issueNumberFromIdentifier(issueIdentifier);
      if (!issueNumber) {{
        return null;
      }}

      return projectBase.replace(/\/+$/, '') + '/' + issueNumber;
    }}

    function renderRunningTable(running, sessionInfoByIssue, trackerProjectUrl) {{
      const rows = Object.entries(running || {{}}).sort(function(a, b) {{
        return String(a[1].issue_identifier || '').localeCompare(String(b[1].issue_identifier || ''));
      }});

      if (rows.length === 0) {{
        return '<tr><td class="muted" colspan="13">No running sessions.</td></tr>';
      }}

      return rows.map(function(entry) {{
        const issueId = entry[0];
        const run = entry[1];
        const sessionInfo = (sessionInfoByIssue && sessionInfoByIssue[issueId]) || {{}};
        const startedAt = Date.parse(run.started_at || '');
        const elapsed = Number.isFinite(startedAt) ? formatDuration(Date.now() - startedAt) : 'n/a';
        const attempt = run.attempt ?? 1;
        const maxTurns = Number(sessionInfo.max_turns);
        const turnCount = Number(sessionInfo.turn_count);
        const turnLabel = Number.isFinite(turnCount) && turnCount > 0
          ? String(turnCount) + '/' + (Number.isFinite(maxTurns) && maxTurns > 0 ? String(maxTurns) : '?')
          : 'n/a';
        const lastActivityValue = sessionInfo.last_activity_ms;
        const lastActivityMs = lastActivityValue != null ? Number(lastActivityValue) : NaN;
        const lastActivityAge = Number.isFinite(lastActivityMs) ? Math.max(0, Date.now() - lastActivityMs) : null;
        const lastActivityLabel = lastActivityAge === null ? 'n/a' : formatTimeAgo(lastActivityAge);
        const lastActivityClass = lastActivityAge !== null && lastActivityAge > STALE_ACTIVITY_THRESHOLD_MS
          ? 'mono stale-activity'
          : 'mono';
        const sessionTokens = sessionInfo.session_tokens || {{}};
        const tokenInput = Number(sessionTokens.input_tokens ?? 0);
        const tokenOutput = Number(sessionTokens.output_tokens ?? 0);
        const tokenTotal = Number(sessionTokens.total_tokens ?? 0);
        const tokenLabel = tokenInput + ' / ' + tokenOutput + ' / ' + tokenTotal;
        const toolName = sessionInfo.current_tool_name || '';
        const toolArgs = sessionInfo.current_tool_args_preview || '';
        const activityLabel = toolName ? 'tool: ' + toolName + (toolArgs ? ' (' + toolArgs + ')' : '') : '-';
        const issueIdentifier = run.issue_identifier || '-';
        const issueUrl = buildIssueUrl(issueIdentifier, run.issue_url, trackerProjectUrl);
        const issueCell = issueUrl
          ? '<a href="' + escapeHtml(issueUrl) + '" target="_blank" rel="noopener noreferrer">' + escapeHtml(issueIdentifier) + '</a>'
          : escapeHtml(issueIdentifier);
        const errorValue = sessionInfo.last_error;
        const errorCell = errorValue
          ? '<td class="mono error-text">' + escapeHtml(errorValue) + '</td>'
          : '<td class="muted">-</td>';
        return '<tr>' +
          '<td class="mono">' + issueCell + '</td>' +
          '<td>' + escapeHtml(run.tracker_state || '-') + '</td>' +
          '<td>' + escapeHtml(run.status || '-') + '</td>' +
          '<td class="mono">' + escapeHtml(activityLabel) + '</td>' +
          errorCell +
          '<td>' + escapeHtml(attempt) + '</td>' +
          '<td class="mono">' + escapeHtml(turnLabel) + '</td>' +
          '<td class="' + escapeHtml(lastActivityClass) + '">' + escapeHtml(lastActivityLabel) + '</td>' +
          '<td class="mono">' + escapeHtml(elapsed) + '</td>' +
          '<td class="mono">' + escapeHtml(tokenLabel) + '</td>' +
          '<td class="mono">' + escapeHtml(run.model || '-') + '</td>' +
          '<td class="mono">' + escapeHtml(run.workspace_path || '-') + '</td>' +
          '<td>' + escapeHtml(run.worker_host || 'local') + '</td>' +
          '</tr>';
      }}).join('');
    }}

    function renderEscalationTable(escalations, running) {{
      const rows = Array.isArray(escalations) ? escalations.slice() : [];
      const runningRows = running && typeof running === 'object' ? Object.values(running) : [];

      rows.sort(function(a, b) {{
        return Date.parse(a.created_at || '') - Date.parse(b.created_at || '');
      }});

      if (rows.length === 0) {{
        return '<tr><td class="muted" colspan="5">No pending escalations.</td></tr>';
      }}

      return rows.map(function(entry) {{
        const createdAt = Date.parse(entry.created_at || '');
        const waitingMs = Number.isFinite(createdAt) ? Math.max(0, Date.now() - createdAt) : null;
        const timeoutMs = Number(entry.timeout_ms ?? 0);
        const remainingMs = timeoutMs > 0 && waitingMs !== null ? Math.max(0, timeoutMs - waitingMs) : null;
        const timeoutLabel = remainingMs === null
          ? 'n/a'
          : (remainingMs === 0 ? 'expired' : ('expires in ' + formatDuration(remainingMs)));

        const label = entry.issue_identifier || entry.issue_id || '-';
        const relatedRun = runningRows.find(function(run) {{
          return run && run.issue_id === entry.issue_id;
        }});
        const issueUrl = relatedRun && typeof relatedRun.issue_url === 'string' ? relatedRun.issue_url : null;
        const issueCell = issueUrl
          ? '⚠️ <a href="' + escapeHtml(issueUrl) + '" target="_blank" rel="noopener noreferrer">' + escapeHtml(label) + '</a>'
          : '⚠️ ' + escapeHtml(label);

        return '<tr>' +
          '<td class="mono">' + issueCell + '</td>' +
          '<td class="mono">' + escapeHtml(entry.method || '-') + '</td>' +
          '<td>' + escapeHtml(entry.preview || '-') + '</td>' +
          '<td class="mono">' + escapeHtml(waitingMs === null ? 'n/a' : formatTimeAgo(waitingMs)) + '</td>' +
          '<td class="mono">' + escapeHtml(timeoutLabel) + '</td>' +
          '</tr>';
      }}).join('');
    }}

    function renderRetryTable(retryQueue) {{
      const rows = Array.isArray(retryQueue) ? retryQueue.slice() : [];

      function retryDelayValue(entry) {{
        return entry.retry_after_ms ?? entry.due_in_ms;
      }}

      rows.sort(function(a, b) {{
        return Number(retryDelayValue(a) ?? 0) - Number(retryDelayValue(b) ?? 0);
      }});

      if (rows.length === 0) {{
        return '<tr><td class="muted" colspan="5">No pending retries.</td></tr>';
      }}

      return rows.map(function(retry) {{
        const retryAfterMs = retryDelayValue(retry);
        return '<tr>' +
          '<td class="mono">' + escapeHtml(retry.identifier || '-') + '</td>' +
          '<td>' + escapeHtml(retry.attempt ?? '-') + '</td>' +
          '<td>' + escapeHtml(retry.error || '-') + '</td>' +
          '<td class="mono">' + escapeHtml(formatRetryDelay(retryAfterMs)) + '</td>' +
          '<td>' + escapeHtml(retry.worker_host || 'local') + '</td>' +
          '</tr>';
      }}).join('');
    }}

    function renderCompleted(completed, trackerProjectUrl) {{
      const items = Array.isArray(completed) ? completed : [];
      if (items.length === 0) {{
        return '<li class="muted">No completed issues yet.</li>';
      }}

      return items.map(function(entry) {{
        const id = entry.identifier || entry.issue_id || '-';
        const issueUrl = buildIssueUrl(id, entry.issue_url, trackerProjectUrl);
        const idLabel = issueUrl
          ? '<a href="' + escapeHtml(issueUrl) + '" target="_blank" rel="noopener noreferrer">' + escapeHtml(id) + '</a>'
          : escapeHtml(id);
        const title = entry.title || '';
        const date = entry.completed_at ? new Date(entry.completed_at).toLocaleString() : '';
        const titleSuffix = title ? ' — ' + escapeHtml(title) : '';
        const suffix = date ? ' <span class="muted" style="font-size:12px">(' + escapeHtml(date) + ')</span>' : '';
        return '<li class="mono">' + idLabel + titleSuffix + suffix + '</li>';
      }}).join('');
    }}

    function formatContextScope(scope) {{
      if (!scope || typeof scope !== 'object') return '-';
      const type = scope.type || '';
      if (type === 'project') return 'project';
      if (type === 'milestone') return 'milestone:' + (scope.value || '-');
      if (type === 'label') return 'label:' + (scope.value || '-');
      return '-';
    }}

    function contextAgeAndTtl(entry, nowMs) {{
      const createdAtMs = Date.parse(entry.created_at || '');
      if (!Number.isFinite(createdAtMs)) {{
        return {{ age: 'n/a', ttlRemaining: 'n/a' }};
      }}

      const ageMs = Math.max(0, nowMs - createdAtMs);
      const ttlMs = Number(entry.ttl_ms || 0);
      const ttlRemainingMs = Number.isFinite(ttlMs) && ttlMs > 0
        ? Math.max(0, ttlMs - ageMs)
        : NaN;

      return {{
        age: formatTimeAgo(ageMs),
        ttlRemaining: Number.isFinite(ttlRemainingMs) ? formatDuration(ttlRemainingMs) : 'n/a',
      }};
    }}

    function renderSharedContext(entries) {{
      const list = Array.isArray(entries) ? entries.slice() : [];
      list.sort(function(a, b) {{
        return Date.parse(b.created_at || '') - Date.parse(a.created_at || '');
      }});

      if (list.length === 0) {{
        return '<tr><td class="muted" colspan="5">No active shared context entries.</td></tr>';
      }}

      const nowMs = Date.now();
      return list.map(function(entry) {{
        const timing = contextAgeAndTtl(entry, nowMs);
        return '<tr>' +
          '<td class="mono">' + escapeHtml(entry.author_issue || '-') + '</td>' +
          '<td class="mono">' + escapeHtml(formatContextScope(entry.scope)) + '</td>' +
          '<td>' + escapeHtml(entry.content || '-') + '</td>' +
          '<td class="mono">' + escapeHtml(timing.age) + '</td>' +
          '<td class="mono">' + escapeHtml(timing.ttlRemaining) + '</td>' +
          '</tr>';
      }}).join('');
    }}

    function updateSharedContextSection(entries) {{
      const list = Array.isArray(entries) ? entries : [];
      document.getElementById('context-count').textContent = String(list.length);
      document.getElementById('context-table-body').innerHTML = renderSharedContext(list);

      const details = document.getElementById('shared-context-details');
      if (details) {{
        details.open = list.length <= 5;
      }}
    }}

    function renderTrackerProjectCard(url) {{
      if (!url) {{
        return '<section class="card"><div class="label">project</div><div class="mono muted">n/a</div></section>';
      }}
      const escaped = escapeHtml(url);
      return '<section class="card"><div class="label">project</div><div class="mono"><a id="tracker-project-link" href="' +
        escaped +
        '" target="_blank" rel="noopener noreferrer">' +
        escaped +
        '</a></div></section>';
    }}

    function formatSupervisorStatus(supervisor) {{
      const status = (supervisor && supervisor.status) || 'disabled';
      if (status === 'active') return '🟢 active';
      if (status === 'starting') return '🟡 starting';
      if (status === 'failed') return '🔴 failed';
      if (status === 'stopped') return '⚫ stopped';
      return '⚪ disabled';
    }}

    function updateSupervisor(supervisor) {{
      const data = supervisor || {{}};
      const status = formatSupervisorStatus(data);
      document.getElementById('supervisor-status').textContent = status;
      document.getElementById('supervisor-status-detail').textContent = status;
      document.getElementById('supervisor-steers').textContent = String(data.steers_issued || 0);
      document.getElementById('supervisor-conflicts').textContent = String(data.conflicts_detected || 0);
      document.getElementById('supervisor-patterns').textContent = String(data.patterns_detected || 0);
      document.getElementById('supervisor-escalations').textContent = String(data.escalations_created || 0);
      document.getElementById('supervisor-last-decision').textContent = data.last_decision || 'n/a';
      document.getElementById('supervisor-last-action').textContent = data.last_action_at ? formatDate(data.last_action_at) : 'n/a';
    }}

    function updatePolling(polling) {{
      const poll = polling || {{}};
      document.getElementById('polling-last-poll').textContent = formatDate(poll.last_poll_at);
      document.getElementById('polling-count').textContent =
        poll.poll_count === undefined || poll.poll_count === null ? 'n/a' : String(poll.poll_count);
      document.getElementById('polling-checking').textContent = poll.checking ? 'yes' : 'no';
      document.getElementById('polling-next-poll').textContent = formatDuration(poll.next_poll_in_ms);
      document.getElementById('polling-interval').textContent = formatDuration(poll.poll_interval_ms);
    }}

    function clearError() {{
      const error = document.getElementById('refresh-error');
      error.hidden = true;
      error.textContent = '';
    }}

    function showError(message) {{
      const error = document.getElementById('refresh-error');
      error.hidden = false;
      error.textContent = message;
    }}

    async function refreshState() {{
      try {{
        const [stateResponse, contextResponse] = await Promise.all([
          fetch('/api/v1/state'),
          fetch('/api/v1/context'),
        ]);
        if (!stateResponse.ok) throw new Error('state fetch failed: ' + stateResponse.status);
        if (!contextResponse.ok) throw new Error('context fetch failed: ' + contextResponse.status);

        const state = await stateResponse.json();
        const contextPayload = await contextResponse.json();
        const running = state.running || {{}};
        const pendingEscalations = state.pending_escalations || [];
        const retryQueue = state.retry_queue || [];
        const completed = state.completed || [];
        const sharedContextEntries = contextPayload.entries || [];

        document.getElementById('running-count').textContent = Object.keys(running).length;
        document.getElementById('escalation-count').textContent = pendingEscalations.length;
        document.getElementById('retry-count').textContent = retryQueue.length;
        document.getElementById('claimed-count').textContent = (state.claimed || []).length;
        document.getElementById('completed-count').textContent = completed.length;
        document.getElementById('tracker-project-card').innerHTML =
          renderTrackerProjectCard(state.tracker_project_url);
        document.getElementById('running-table-body').innerHTML = renderRunningTable(
          running,
          state.running_session_info || {{}},
          state.tracker_project_url
        );
        document.getElementById('escalation-table-body').innerHTML = renderEscalationTable(pendingEscalations, running);
        document.getElementById('retry-table-body').innerHTML = renderRetryTable(retryQueue);

        const blocked = state.blocked || [];
        const blockedSection = document.getElementById('blocked-section');
        if (blocked.length > 0) {{
          blockedSection.style.display = '';
          document.getElementById('blocked-table-body').innerHTML = blocked.map(function(entry) {{
            return '<tr><td>' + escapeHtml(entry.identifier) + '</td>'
              + '<td>' + escapeHtml(entry.title || '') + '</td>'
              + '<td>' + escapeHtml(entry.state || '') + '</td>'
              + '<td>' + escapeHtml((entry.blocker_identifiers || []).join(', ')) + '</td></tr>';
          }}).join('');
        }} else {{
          blockedSection.style.display = 'none';
        }}

        document.getElementById('completed-list').innerHTML = renderCompleted(
          completed,
          state.tracker_project_url
        );
        updateSharedContextSection(sharedContextEntries);
        updateSupervisor(state.supervisor || {{}});
        document.getElementById('token-input').textContent = state.codex_totals?.input_tokens ?? 0;
        document.getElementById('token-output').textContent = state.codex_totals?.output_tokens ?? 0;
        document.getElementById('token-total').textContent = state.codex_totals?.total_tokens ?? 0;
        document.getElementById('rate-limits').textContent = JSON.stringify(state.codex_rate_limits || {{}}, null, 2);
        updatePolling(state.polling);
        clearError();
      }} catch (error) {{
        showError(String(error));
      }}
    }}
    refreshState();
    setInterval(refreshState, 2000);
  </script>
</body>
</html>
"#
    );

    Html(html)
}

async fn get_state(State(state): State<HttpServerState>) -> impl IntoResponse {
    let snapshot = state.snapshot();
    Json(StateResponse {
        poll_interval_ms: snapshot.poll_interval_ms,
        max_concurrent_agents: snapshot.max_concurrent_agents,
        tracker_project_url: snapshot.tracker_project_url,
        running: snapshot.running,
        running_sessions: snapshot.running_sessions,
        running_session_info: snapshot.running_session_info,
        claimed: snapshot.claimed,
        retry_queue: snapshot.retry_queue,
        blocked: snapshot.blocked,
        pending_escalations: snapshot.pending_escalations,
        completed: snapshot.completed,
        shared_context: snapshot.shared_context,
        supervisor: snapshot.supervisor,
        codex_totals: snapshot.codex_totals,
        codex_rate_limits: snapshot.codex_rate_limits.map(|r| r.data),
        polling: snapshot.polling,
    })
}

async fn get_escalations(State(state): State<HttpServerState>) -> impl IntoResponse {
    Json(EscalationPendingResponse {
        pending: state.pending_escalations(),
    })
}

async fn post_escalation_respond(
    Path(request_id): Path<String>,
    State(state): State<HttpServerState>,
    Json(body): Json<EscalationRespondRequest>,
) -> impl IntoResponse {
    let result = state.resolve_escalation(&request_id, body.response, body.responder_id);

    match result {
        EscalationResolveResult::Resolved => {
            (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
        }
        EscalationResolveResult::NotFound => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "escalation_not_found" })),
        )
            .into_response(),
        EscalationResolveResult::AlreadyResolved => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "escalation_already_resolved" })),
        )
            .into_response(),
    }
}

fn parse_shared_context_scopes(
    raw: Option<&str>,
) -> std::result::Result<Vec<ContextScope>, ApiErrorEnvelope> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };

    let mut scopes = Vec::new();
    for token in raw.split(',') {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return Err(ApiErrorEnvelope {
                error: ApiError {
                    code: "invalid_scope",
                    message: "shared context scope filter cannot contain empty values".to_string(),
                    status: StatusCode::BAD_REQUEST.as_u16(),
                    details: Some(serde_json::json!({
                        "allowed": ["project", "milestone:<id>", "label:<name>"],
                    })),
                },
            });
        }

        let Some(scope) = ContextScope::parse(trimmed) else {
            return Err(ApiErrorEnvelope {
                error: ApiError {
                    code: "invalid_scope",
                    message: format!(
                        "invalid shared context scope '{trimmed}'. Use project, milestone:<id>, or label:<name>"
                    ),
                    status: StatusCode::BAD_REQUEST.as_u16(),
                    details: Some(serde_json::json!({
                        "scope": trimmed,
                        "allowed": ["project", "milestone:<id>", "label:<name>"],
                    })),
                },
            });
        };

        scopes.push(scope);
    }

    scopes.sort();
    scopes.dedup();
    Ok(scopes)
}

fn shared_context_preview(value: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 120;
    value.chars().take(MAX_PREVIEW_CHARS).collect()
}

async fn get_context(
    Query(query): Query<SharedContextQuery>,
    State(state): State<HttpServerState>,
) -> impl IntoResponse {
    let scopes = match parse_shared_context_scopes(query.scope.as_deref()) {
        Ok(scopes) => scopes,
        Err(err) => return (StatusCode::BAD_REQUEST, Json(err)).into_response(),
    };

    let store = state.shared_context_store();
    let entries = if scopes.is_empty() {
        store.list()
    } else {
        store.read(&scopes)
    };

    Json(SharedContextListResponse {
        entries,
        summary: store.summary(),
    })
    .into_response()
}

async fn post_context(
    State(state): State<HttpServerState>,
    Json(payload): Json<SharedContextWriteRequest>,
) -> impl IntoResponse {
    let content_len = payload.content.chars().count();
    if content_len == 0 || payload.content.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "invalid_content",
                    message: "shared context content must be non-empty".to_string(),
                    status: StatusCode::BAD_REQUEST.as_u16(),
                    details: None,
                },
            }),
        )
            .into_response();
    }

    if content_len > MAX_SHARED_CONTEXT_CONTENT_CHARS {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "invalid_content",
                    message: format!(
                        "shared context content must be {} characters or fewer",
                        MAX_SHARED_CONTEXT_CONTENT_CHARS
                    ),
                    status: StatusCode::BAD_REQUEST.as_u16(),
                    details: Some(serde_json::json!({
                        "max_chars": MAX_SHARED_CONTEXT_CONTENT_CHARS,
                        "actual_chars": content_len,
                    })),
                },
            }),
        )
            .into_response();
    }

    let scope = match ContextScope::parse(&payload.scope) {
        Some(scope) => scope,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiErrorEnvelope {
                    error: ApiError {
                        code: "invalid_scope",
                        message: format!(
                            "invalid shared context scope '{}'. Use project, milestone:<id>, or label:<name>",
                            payload.scope
                        ),
                        status: StatusCode::BAD_REQUEST.as_u16(),
                        details: Some(serde_json::json!({
                            "scope": payload.scope,
                            "allowed": ["project", "milestone:<id>", "label:<name>"],
                        })),
                    },
                }),
            )
                .into_response();
        }
    };

    if payload.ttl_ms == Some(0) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "invalid_ttl",
                    message: "ttl_ms must be greater than 0 when provided".to_string(),
                    status: StatusCode::BAD_REQUEST.as_u16(),
                    details: Some(serde_json::json!({
                        "ttl_ms": 0,
                    })),
                },
            }),
        )
            .into_response();
    }

    let store = state.shared_context_store();
    let entry = store.write_entry(ContextEntryDraft {
        author_issue: payload.author_issue,
        scope,
        content: payload.content,
        ttl_ms: payload.ttl_ms,
    });

    state.event_hub().publish_with_timestamp(
        EventKind::SharedContextWritten,
        EventSeverity::Info,
        Some(entry.author_issue.clone()),
        "shared_context_written",
        serde_json::json!({
            "entry_id": entry.id,
            "author_issue": entry.author_issue,
            "scope": entry.scope.as_scope_key(),
            "preview": shared_context_preview(&entry.content),
        }),
        entry.created_at,
    );

    (
        StatusCode::CREATED,
        Json(SharedContextWriteResponse {
            id: entry.id,
            created_at: entry.created_at,
        }),
    )
        .into_response()
}

async fn delete_context(
    Query(query): Query<SharedContextQuery>,
    State(state): State<HttpServerState>,
) -> impl IntoResponse {
    let scopes = match parse_shared_context_scopes(query.scope.as_deref()) {
        Ok(scopes) => scopes,
        Err(err) => return (StatusCode::BAD_REQUEST, Json(err)).into_response(),
    };

    let deleted = if scopes.is_empty() {
        state.shared_context_store().clear(None)
    } else {
        state.shared_context_store().clear(Some(&scopes))
    };

    Json(SharedContextDeleteResponse { deleted }).into_response()
}

async fn delete_context_entry(
    Path(entry_id): Path<String>,
    State(state): State<HttpServerState>,
) -> impl IntoResponse {
    let store = state.shared_context_store();
    if store.remove_entry(&entry_id).is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "context_not_found",
                    message: format!("shared context entry '{}' was not found", entry_id),
                    status: StatusCode::NOT_FOUND.as_u16(),
                    details: None,
                },
            }),
        )
            .into_response();
    }

    Json(SharedContextDeleteResponse { deleted: 1 }).into_response()
}

async fn get_events(
    ws: WebSocketUpgrade,
    Query(query): Query<EventFilterQuery>,
    State(state): State<HttpServerState>,
) -> impl IntoResponse {
    let filter = match parse_event_filter(&query) {
        Ok(filter) => filter,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(err.to_api_error())).into_response();
        }
    };

    ws.on_upgrade(move |socket| handle_events_socket(socket, state, filter))
}

async fn handle_events_socket(socket: WebSocket, state: HttpServerState, filter: EventFilter) {
    let client_id = state.next_client_id();
    let connected_count = state.increment_connected();
    tracing::info!(
        event = "ws_client_connected",
        client_id,
        connected = connected_count,
        filter = ?filter,
        "event stream websocket client connected"
    );

    let event_hub = state.event_hub();
    let config = state.event_stream_config();
    let mut broadcast_rx = event_hub.subscribe();

    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (outbound_tx, mut outbound_rx) =
        tokio::sync::mpsc::channel::<OutboundMessage>(config.client_queue_capacity.max(1));

    let writer_send_delay = config.writer_send_delay;
    let writer_task = tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            match message {
                OutboundMessage::Envelope(envelope) => {
                    if let Some(delay) = writer_send_delay {
                        tokio::time::sleep(delay).await;
                    }
                    let payload = match serde_json::to_string(&envelope) {
                        Ok(payload) => payload,
                        Err(err) => {
                            tracing::warn!(
                                event = "ws_serialize_failed",
                                error = %err,
                                "failed to serialize websocket event envelope"
                            );
                            continue;
                        }
                    };
                    if ws_sender.send(Message::Text(payload.into())).await.is_err() {
                        break;
                    }
                }
                OutboundMessage::Pong(payload) => {
                    if ws_sender.send(Message::Pong(payload.into())).await.is_err() {
                        break;
                    }
                }
                OutboundMessage::Close(reason) => {
                    let _ = ws_sender
                        .send(Message::Close(Some(reason.close_frame())))
                        .await;
                    break;
                }
            }
        }
    });

    let snapshot = state.snapshot();
    let snapshot_payload = serde_json::to_value(snapshot)
        .unwrap_or_else(|_| serde_json::json!({ "error": "snapshot_serialization_failed" }));
    let snapshot_envelope = SymphonyEventEnvelope::new(
        event_hub.next_sequence(),
        Utc::now(),
        EventKind::Snapshot,
        EventSeverity::Info,
        None,
        "snapshot",
        snapshot_payload,
    );

    let mut sent_count: u64 = 0;
    let mut dropped_count: u64 = 0;
    let mut close_reason = WsCloseReason::ClientClosed;

    if enqueue_event(
        &state,
        &outbound_tx,
        client_id,
        snapshot_envelope,
        &mut dropped_count,
        config.backpressure_drop_threshold,
    ) {
        sent_count = sent_count.saturating_add(1);
    } else {
        close_reason = WsCloseReason::Backpressure;
    }

    let mut heartbeat = tokio::time::interval(config.heartbeat_interval);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    while close_reason == WsCloseReason::ClientClosed {
        tokio::select! {
            incoming = ws_receiver.next() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => {
                        close_reason = WsCloseReason::ClientClosed;
                        break;
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if !enqueue_pong(
                            &state,
                            &outbound_tx,
                            client_id,
                            payload.to_vec(),
                            &mut dropped_count,
                        ) {
                            close_reason = WsCloseReason::Backpressure;
                            break;
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        tracing::warn!(
                            event = "ws_client_protocol_error",
                            client_id,
                            error = %err,
                            "websocket receive error"
                        );
                        close_reason = WsCloseReason::ProtocolError;
                        break;
                    }
                }
            }
            broadcast = broadcast_rx.recv() => {
                match broadcast {
                    Ok(envelope) => {
                        if filter.matches(&envelope) {
                            if enqueue_event(
                                &state,
                                &outbound_tx,
                                client_id,
                                envelope,
                                &mut dropped_count,
                                config.backpressure_drop_threshold,
                            ) {
                                sent_count = sent_count.saturating_add(1);
                            } else {
                                close_reason = WsCloseReason::Backpressure;
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        dropped_count = dropped_count.saturating_add(skipped);
                        let total_dropped = state.increment_dropped(skipped);
                        tracing::warn!(
                            event = "ws_event_dropped",
                            client_id,
                            reason = "lagged",
                            dropped = skipped,
                            dropped_total = dropped_count,
                            dropped_global = total_dropped,
                            "client lagged behind websocket event hub"
                        );
                        if dropped_count >= config.backpressure_drop_threshold {
                            close_reason = WsCloseReason::Backpressure;
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        close_reason = WsCloseReason::ServerShutdown;
                        break;
                    }
                }
            }
            _ = heartbeat.tick() => {
                let heartbeat_envelope = SymphonyEventEnvelope::new(
                    event_hub.next_sequence(),
                    Utc::now(),
                    EventKind::Heartbeat,
                    EventSeverity::Debug,
                    None,
                    "heartbeat",
                    serde_json::json!({ "client_id": client_id }),
                );

                if enqueue_event(
                    &state,
                    &outbound_tx,
                    client_id,
                    heartbeat_envelope,
                    &mut dropped_count,
                    config.backpressure_drop_threshold,
                ) {
                    sent_count = sent_count.saturating_add(1);
                    let heartbeat_count = state.increment_heartbeat();
                    tracing::debug!(
                        event = "ws_heartbeat_sent",
                        client_id,
                        heartbeat = heartbeat_count,
                        "websocket heartbeat sent"
                    );
                } else {
                    close_reason = WsCloseReason::Backpressure;
                    break;
                }
            }
        }
    }

    match tokio::time::timeout(
        Duration::from_millis(WS_CLOSE_ENQUEUE_TIMEOUT_MS),
        outbound_tx.send(OutboundMessage::Close(close_reason)),
    )
    .await
    {
        Ok(Ok(())) | Ok(Err(_)) => {}
        Err(_) => {
            tracing::warn!(
                event = "ws_close_enqueue_timeout",
                client_id,
                reason = close_reason.as_str(),
                timeout_ms = WS_CLOSE_ENQUEUE_TIMEOUT_MS,
                "timed out enqueueing websocket close frame; aborting writer task"
            );
            writer_task.abort();
        }
    }
    drop(outbound_tx);
    let _ = writer_task.await;

    let disconnected_count = state.increment_disconnected();
    tracing::info!(
        event = "ws_client_disconnected",
        client_id,
        reason = close_reason.as_str(),
        sent = sent_count,
        dropped = dropped_count,
        disconnected = disconnected_count,
        "event stream websocket client disconnected"
    );
}

fn enqueue_event(
    state: &HttpServerState,
    outbound_tx: &tokio::sync::mpsc::Sender<OutboundMessage>,
    client_id: u64,
    envelope: SymphonyEventEnvelope,
    dropped_count: &mut u64,
    drop_threshold: u64,
) -> bool {
    enqueue_outbound(
        state,
        outbound_tx,
        client_id,
        OutboundMessage::Envelope(envelope),
        dropped_count,
        drop_threshold,
        "queue_full",
    )
}

fn enqueue_pong(
    state: &HttpServerState,
    outbound_tx: &tokio::sync::mpsc::Sender<OutboundMessage>,
    client_id: u64,
    payload: Vec<u8>,
    dropped_count: &mut u64,
) -> bool {
    match outbound_tx.try_send(OutboundMessage::Pong(payload)) {
        Ok(()) => true,
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            *dropped_count = dropped_count.saturating_add(1);
            let global_dropped = state.increment_dropped(1);
            tracing::warn!(
                event = "ws_event_dropped",
                client_id,
                reason = "queue_full_pong",
                dropped_total = *dropped_count,
                dropped_global = global_dropped,
                should_close = true,
                "websocket pong frame dropped; closing connection"
            );
            false
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => false,
    }
}

fn enqueue_outbound(
    state: &HttpServerState,
    outbound_tx: &tokio::sync::mpsc::Sender<OutboundMessage>,
    client_id: u64,
    message: OutboundMessage,
    dropped_count: &mut u64,
    drop_threshold: u64,
    drop_reason: &'static str,
) -> bool {
    let effective_drop_threshold = drop_threshold.max(1);

    match outbound_tx.try_send(message) {
        Ok(()) => true,
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            *dropped_count = dropped_count.saturating_add(1);
            let global_dropped = state.increment_dropped(1);
            let should_close = *dropped_count >= effective_drop_threshold;
            tracing::warn!(
                event = "ws_event_dropped",
                client_id,
                reason = drop_reason,
                dropped_total = *dropped_count,
                dropped_global = global_dropped,
                drop_threshold = effective_drop_threshold,
                should_close,
                "websocket client outbound queue reached capacity"
            );
            !should_close
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => false,
    }
}

pub fn parse_event_filter_contract(
    issue: Option<&str>,
    event_type: Option<&str>,
    severity: Option<&str>,
) -> Result<EventFilter, EventFilterError> {
    parse_event_filter(&EventFilterQuery {
        issue: issue.map(str::to_string),
        event_type: event_type.map(str::to_string),
        severity: severity.map(str::to_string),
    })
}

fn parse_event_filter(query: &EventFilterQuery) -> Result<EventFilter, EventFilterError> {
    let issues = parse_issue_filter(query.issue.as_deref())?;
    let kinds = parse_kind_filter(query.event_type.as_deref())?;
    let severities = parse_severity_filter(query.severity.as_deref())?;

    Ok(EventFilter {
        issues,
        kinds,
        severities,
    })
}

fn parse_issue_filter(raw: Option<&str>) -> Result<BTreeSet<String>, EventFilterError> {
    let Some(raw) = raw else {
        return Ok(BTreeSet::new());
    };

    let mut issues = BTreeSet::new();
    for token in raw.split(',') {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return Err(EventFilterError {
                field: "issue",
                value: token.to_string(),
                message: "invalid issue filter: empty issue value".to_string(),
            });
        }

        let normalized = trimmed.to_ascii_uppercase();
        if !looks_like_issue_identifier(&normalized) {
            return Err(EventFilterError {
                field: "issue",
                value: token.to_string(),
                message: format!(
                    "invalid issue filter value '{trimmed}': expected TEAM-123 style identifier"
                ),
            });
        }
        issues.insert(normalized);
    }

    Ok(issues)
}

fn parse_kind_filter(raw: Option<&str>) -> Result<BTreeSet<EventKind>, EventFilterError> {
    let Some(raw) = raw else {
        return Ok(BTreeSet::new());
    };

    let mut kinds = BTreeSet::new();
    for token in raw.split(',') {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return Err(EventFilterError {
                field: "type",
                value: token.to_string(),
                message: "invalid type filter: empty event kind".to_string(),
            });
        }

        let Some(kind) = EventKind::parse(trimmed) else {
            return Err(EventFilterError {
                field: "type",
                value: trimmed.to_string(),
                message: format!(
                    "invalid type filter value '{trimmed}'. Allowed values: {}",
                    EventKind::variants().join(",")
                ),
            });
        };
        kinds.insert(kind);
    }

    Ok(kinds)
}

fn parse_severity_filter(raw: Option<&str>) -> Result<BTreeSet<EventSeverity>, EventFilterError> {
    let Some(raw) = raw else {
        return Ok(BTreeSet::new());
    };

    let mut severities = BTreeSet::new();
    for token in raw.split(',') {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return Err(EventFilterError {
                field: "severity",
                value: token.to_string(),
                message: "invalid severity filter: empty severity".to_string(),
            });
        }

        let Some(severity) = EventSeverity::parse(trimmed) else {
            return Err(EventFilterError {
                field: "severity",
                value: trimmed.to_string(),
                message: format!(
                    "invalid severity filter value '{trimmed}'. Allowed values: {}",
                    EventSeverity::variants().join(",")
                ),
            });
        };
        severities.insert(severity);
    }

    Ok(severities)
}

async fn get_factory_run_by_id(
    State(state): State<HttpServerState>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    let Some(query) = state.factory_run_query.as_ref() else {
        return factory_store_unavailable().into_response();
    };

    match query.get_run(run_id.trim()) {
        Ok(Some(run)) => Json(run).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "factory_run_not_found",
                    message: format!("Factory run '{run_id}' was not found"),
                    status: StatusCode::NOT_FOUND.as_u16(),
                    details: None,
                },
            }),
        )
            .into_response(),
        Err(message) => factory_query_failed(&message).into_response(),
    }
}

async fn get_factory_artifact(
    State(state): State<HttpServerState>,
    Path((run_id, artifact_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let Some(query) = state.factory_run_query.as_ref() else {
        return factory_store_unavailable().into_response();
    };
    match query.get_artifact(run_id.trim(), artifact_id.trim()) {
        Ok(Some(artifact)) => Json(artifact).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "factory_artifact_not_found",
                    message: format!("Factory artifact '{artifact_id}' was not found"),
                    status: StatusCode::NOT_FOUND.as_u16(),
                    details: None,
                },
            }),
        )
            .into_response(),
        Err(message) => factory_query_failed(&message).into_response(),
    }
}

async fn get_factory_runs(
    State(state): State<HttpServerState>,
    Query(params): Query<FactoryRunsListQuery>,
) -> impl IntoResponse {
    let Some(query) = state.factory_run_query.as_ref() else {
        return factory_store_unavailable().into_response();
    };

    let Some(issue) = params
        .issue
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "invalid_query",
                    message: "Query parameter 'issue' is required".to_string(),
                    status: StatusCode::BAD_REQUEST.as_u16(),
                    details: Some(serde_json::json!({ "field": "issue" })),
                },
            }),
        )
            .into_response();
    };

    if !looks_like_factory_issue_identifier(issue) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "invalid_query",
                    message: format!("Query parameter 'issue' value '{issue}' is invalid"),
                    status: StatusCode::BAD_REQUEST.as_u16(),
                    details: Some(serde_json::json!({ "field": "issue", "value": issue })),
                },
            }),
        )
            .into_response();
    }

    match query.get_run_by_issue(issue) {
        Ok(run) => Json(run).into_response(),
        Err(message) => factory_query_failed(&message).into_response(),
    }
}

async fn get_factory_run_metrics(
    State(state): State<HttpServerState>,
    Query(params): Query<FactoryRunsMetricsQuery>,
) -> impl IntoResponse {
    let Some(query) = state.factory_run_query.as_ref() else {
        return factory_store_unavailable().into_response();
    };

    let stage = params.stage.as_deref().map(str::trim).unwrap_or("");
    let result = match stage {
        crate::triage::domain::TRIAGE_STAGE_NAME => query.triage_metrics(),
        crate::spec::domain::SPEC_STAGE_NAME => query.spec_metrics(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiErrorEnvelope {
                    error: ApiError {
                        code: "invalid_query",
                        message: "Query parameter 'stage' must be 'triage' or 'spec'".to_string(),
                        status: StatusCode::BAD_REQUEST.as_u16(),
                        details: Some(serde_json::json!({
                            "field": "stage",
                            "value": params.stage,
                        })),
                    },
                }),
            )
                .into_response();
        }
    };

    match result {
        Ok(metrics) => Json(metrics).into_response(),
        Err(message) => factory_query_failed(&message).into_response(),
    }
}

fn factory_store_unavailable() -> (StatusCode, Json<ApiErrorEnvelope>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ApiErrorEnvelope {
            error: ApiError {
                code: "factory_store_unavailable",
                message: "Factory run store is not configured".to_string(),
                status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                details: None,
            },
        }),
    )
}

fn factory_query_failed(message: &str) -> (StatusCode, Json<ApiErrorEnvelope>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiErrorEnvelope {
            error: ApiError {
                code: "factory_query_failed",
                message: message.to_string(),
                status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                details: None,
            },
        }),
    )
}

fn looks_like_factory_issue_identifier(candidate: &str) -> bool {
    let trimmed = candidate.trim();
    if trimmed.is_empty() || trimmed.len() > 200 {
        return false;
    }
    if looks_like_issue_identifier(trimmed) {
        return true;
    }
    // GitHub-style identifiers: "#123" or "kata#123"
    if let Some(number) = trimmed.strip_prefix('#') {
        return !number.is_empty() && number.chars().all(|c| c.is_ascii_digit());
    }
    if let Some((prefix, number)) = trimmed.rsplit_once('#') {
        return !prefix.is_empty()
            && prefix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            && !number.is_empty()
            && number.chars().all(|c| c.is_ascii_digit());
    }
    false
}

async fn get_issue(
    State(state): State<HttpServerState>,
    Path(issue_identifier): Path<String>,
    uri: Uri,
) -> impl IntoResponse {
    if !looks_like_issue_identifier(&issue_identifier) {
        return api_not_found(uri).await.into_response();
    }

    let snapshot = state.snapshot();

    if let Some(run) = snapshot
        .running
        .values()
        .find(|run| run.issue_identifier == issue_identifier)
    {
        return Json(IssueResponseEnvelope {
            issue: IssueProjection {
                issue_id: run.issue_id.clone(),
                issue_identifier: run.issue_identifier.clone(),
                status: "running",
                attempt: run.attempt,
                error: run.error.clone(),
                worker_host: run.worker_host.clone(),
                workspace_path: Some(run.workspace_path.clone()),
            },
        })
        .into_response();
    }

    if let Some(retry) = snapshot
        .retry_queue
        .iter()
        .find(|retry| retry.identifier == issue_identifier)
    {
        return Json(IssueResponseEnvelope {
            issue: IssueProjection {
                issue_id: retry.issue_id.clone(),
                issue_identifier: retry.identifier.clone(),
                status: "retry",
                attempt: Some(retry.attempt),
                error: retry.error.clone(),
                worker_host: retry.worker_host.clone(),
                workspace_path: retry.workspace_path.clone(),
            },
        })
        .into_response();
    }

    tracing::info!(
        event = "http_issue_not_found",
        issue_identifier = issue_identifier,
        "issue lookup failed in HTTP API"
    );

    (
        StatusCode::NOT_FOUND,
        Json(ApiErrorEnvelope {
            error: ApiError {
                code: "issue_not_found",
                message: format!("Issue identifier '{}' was not found", issue_identifier),
                status: StatusCode::NOT_FOUND.as_u16(),
                details: None,
            },
        }),
    )
        .into_response()
}

async fn post_steer(
    State(state): State<HttpServerState>,
    Json(payload): Json<SteerRequestBody>,
) -> impl IntoResponse {
    let issue_identifier = payload
        .issue_identifier
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_uppercase();
    let instruction = payload
        .instruction
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_string();

    if issue_identifier.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "invalid_request",
                    message: "issue_identifier must be provided".to_string(),
                    status: StatusCode::BAD_REQUEST.as_u16(),
                    details: None,
                },
            }),
        )
            .into_response();
    }

    if instruction.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "invalid_request",
                    message: "instruction must be a non-empty string".to_string(),
                    status: StatusCode::BAD_REQUEST.as_u16(),
                    details: None,
                },
            }),
        )
            .into_response();
    }

    if instruction.chars().count() > MAX_STEER_INSTRUCTION_CHARS {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "instruction_too_long",
                    message: format!(
                        "instruction exceeds max length of {} characters",
                        MAX_STEER_INSTRUCTION_CHARS
                    ),
                    status: StatusCode::BAD_REQUEST.as_u16(),
                    details: None,
                },
            }),
        )
            .into_response();
    }

    let instruction_preview = truncate_preview(&instruction, 100);

    match state
        .request_steer(issue_identifier.clone(), instruction)
        .await
    {
        SteerResult::Delivered {
            issue_id,
            issue_identifier,
        } => (
            StatusCode::OK,
            Json(SteerResponseBody {
                ok: true,
                issue_id,
                issue_identifier,
                delivered: true,
                instruction_preview,
            }),
        )
            .into_response(),
        SteerResult::IssueNotRunning => (
            StatusCode::NOT_FOUND,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "issue_not_running",
                    message: format!("issue '{}' is not currently running", issue_identifier),
                    status: StatusCode::NOT_FOUND.as_u16(),
                    details: None,
                },
            }),
        )
            .into_response(),
        SteerResult::NoActiveSession => (
            StatusCode::CONFLICT,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "no_active_session",
                    message: format!("issue '{}' has no active RPC session", issue_identifier),
                    status: StatusCode::CONFLICT.as_u16(),
                    details: None,
                },
            }),
        )
            .into_response(),
        SteerResult::DeliveryFailed { message } if message == "steer_unavailable" => (
            StatusCode::NOT_IMPLEMENTED,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "capability_unavailable",
                    message: "steer endpoint is not wired to orchestrator runtime".to_string(),
                    status: StatusCode::NOT_IMPLEMENTED.as_u16(),
                    details: None,
                },
            }),
        )
            .into_response(),
        SteerResult::DeliveryFailed { message } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiErrorEnvelope {
                error: ApiError {
                    code: "steer_failed",
                    message,
                    status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    details: None,
                },
            }),
        )
            .into_response(),
    }
}

async fn post_refresh(State(state): State<HttpServerState>) -> impl IntoResponse {
    let outcome = state.request_refresh();

    if outcome.coalesced {
        tracing::info!(
            event = "http_refresh_coalesced",
            pending_requests = outcome.pending_requests,
            "HTTP refresh request coalesced with existing pending refresh"
        );
    } else {
        tracing::info!(
            event = "http_refresh_requested",
            pending_requests = outcome.pending_requests,
            "HTTP refresh request queued"
        );
    }

    (
        StatusCode::ACCEPTED,
        Json(RefreshResponse {
            queued: outcome.queued,
            coalesced: outcome.coalesced,
            pending_requests: outcome.pending_requests,
        }),
    )
}

async fn api_not_found(uri: Uri) -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(ApiErrorEnvelope {
            error: ApiError {
                code: "not_found",
                message: format!("No route found for {}", uri.path()),
                status: StatusCode::NOT_FOUND.as_u16(),
                details: None,
            },
        }),
    )
}

async fn api_method_not_allowed(method: Method, uri: Uri) -> impl IntoResponse {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(ApiErrorEnvelope {
            error: ApiError {
                code: "method_not_allowed",
                message: format!("Method {} is not allowed for {}", method, uri.path()),
                status: StatusCode::METHOD_NOT_ALLOWED.as_u16(),
                details: None,
            },
        }),
    )
}

fn looks_like_issue_identifier(candidate: &str) -> bool {
    let mut parts = candidate.split('-');
    let Some(prefix) = parts.next() else {
        return false;
    };
    let Some(number) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }

    !prefix.is_empty()
        && prefix.chars().all(|c| c.is_ascii_uppercase())
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener as StdTcpListener;

    fn reserve_contiguous_ports(range_width: u16) -> (u16, Vec<StdTcpListener>) {
        for _ in 0..256 {
            let first = StdTcpListener::bind(("127.0.0.1", 0))
                .expect("should bind a seed loopback port for test");
            let base = first
                .local_addr()
                .expect("seed listener should expose local addr")
                .port();

            if u16::MAX - base < range_width {
                continue;
            }

            let mut listeners = vec![first];
            let mut all_reserved = true;
            for offset in 1..=range_width {
                match StdTcpListener::bind(("127.0.0.1", base + offset)) {
                    Ok(listener) => listeners.push(listener),
                    Err(_) => {
                        all_reserved = false;
                        break;
                    }
                }
            }

            if all_reserved {
                return (base, listeners);
            }
        }

        panic!("failed to reserve contiguous loopback ports for test");
    }

    #[tokio::test]
    async fn bind_http_listener_with_fallback_increments_when_configured_port_is_in_use() {
        const MAX_TEST_ATTEMPTS: usize = 5;

        for attempt in 1..=MAX_TEST_ATTEMPTS {
            // Reserve a full contiguous range first so retries are possible, then keep
            // only the configured port occupied while releasing fallback candidates.
            let (configured_port, mut listeners) = reserve_contiguous_ports(HTTP_PORT_RETRY_LIMIT);
            let occupied = listeners.remove(0);
            drop(listeners);

            match bind_http_listener_with_fallback(
                "127.0.0.1",
                configured_port,
                HTTP_PORT_RETRY_LIMIT,
            )
            .await
            {
                Ok((listener, bound_port)) => {
                    assert!(
                        bound_port > configured_port,
                        "bound port should move forward when configured port is taken: configured={configured_port}, bound={bound_port}"
                    );
                    assert!(
                        bound_port <= configured_port.saturating_add(HTTP_PORT_RETRY_LIMIT),
                        "bound port should remain within retry cap: configured={configured_port}, bound={bound_port}"
                    );
                    drop(occupied);
                    drop(listener);
                    return;
                }
                Err(err)
                    if err.kind() == std::io::ErrorKind::AddrInUse
                        && attempt < MAX_TEST_ATTEMPTS =>
                {
                    drop(occupied);
                }
                Err(err) => {
                    drop(occupied);
                    panic!(
                        "fallback bind should find an available nearby port (attempt {attempt}/{MAX_TEST_ATTEMPTS}): {err}"
                    );
                }
            }
        }

        panic!(
            "fallback bind should find an available nearby port after {MAX_TEST_ATTEMPTS} attempts"
        );
    }

    #[tokio::test]
    async fn bind_http_listener_with_fallback_errors_after_retry_cap_is_exhausted() {
        let (configured_port, listeners) = reserve_contiguous_ports(HTTP_PORT_RETRY_LIMIT);

        let err =
            bind_http_listener_with_fallback("127.0.0.1", configured_port, HTTP_PORT_RETRY_LIMIT)
                .await
                .expect_err("binding should fail when configured port and +10 range are occupied");
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);

        drop(listeners);
    }

    #[test]
    fn event_filter_parsing_accepts_or_within_fields_and_and_across_fields() {
        let query = EventFilterQuery {
            issue: Some("KAT-1,kAt-2".to_string()),
            event_type: Some("worker,tool".to_string()),
            severity: Some("info,error".to_string()),
        };

        let filter = parse_event_filter(&query).expect("filter parsing should succeed");
        assert_eq!(
            filter.issues,
            BTreeSet::from(["KAT-1".to_string(), "KAT-2".to_string()])
        );
        assert_eq!(
            filter.kinds,
            BTreeSet::from([EventKind::Worker, EventKind::Tool])
        );
        assert_eq!(
            filter.severities,
            BTreeSet::from([EventSeverity::Info, EventSeverity::Error])
        );

        let matching = SymphonyEventEnvelope::new(
            1,
            Utc::now(),
            EventKind::Tool,
            EventSeverity::Info,
            Some("KAT-2".to_string()),
            "tool_call_completed",
            serde_json::json!({}),
        );
        assert!(filter.matches(&matching));

        let wrong_issue = SymphonyEventEnvelope::new(
            2,
            Utc::now(),
            EventKind::Tool,
            EventSeverity::Info,
            Some("KAT-9".to_string()),
            "tool_call_completed",
            serde_json::json!({}),
        );
        assert!(!filter.matches(&wrong_issue));

        let wrong_type = SymphonyEventEnvelope::new(
            3,
            Utc::now(),
            EventKind::Runtime,
            EventSeverity::Info,
            Some("KAT-2".to_string()),
            "dispatch",
            serde_json::json!({}),
        );
        assert!(!filter.matches(&wrong_type));

        let wrong_severity = SymphonyEventEnvelope::new(
            4,
            Utc::now(),
            EventKind::Worker,
            EventSeverity::Warn,
            Some("KAT-2".to_string()),
            "worker_stalled",
            serde_json::json!({}),
        );
        assert!(!filter.matches(&wrong_severity));
    }

    #[test]
    fn event_filter_parsing_rejects_unknown_type_with_deterministic_error() {
        let query = EventFilterQuery {
            issue: None,
            event_type: Some("worker,wat".to_string()),
            severity: None,
        };

        let err = parse_event_filter(&query).expect_err("unknown event type should fail");
        assert_eq!(err.field, "type");
        assert_eq!(err.value, "wat");
        let allowed_values = format!("Allowed values: {}", EventKind::variants().join(","));
        assert!(
            err.message.contains(&allowed_values),
            "error should list deterministic allowed values"
        );
    }
}
