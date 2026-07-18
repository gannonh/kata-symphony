use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use symphony::domain::{
    AgentEvent, CodexTotals, CompletedEntry, EventKind, EventSeverity, OrchestratorSnapshot,
    PollingSnapshot, RetrySnapshotEntry, RunAttempt, RunningSessionSnapshot, SymphonyEventEnvelope,
    WorkerSessionInfo,
};
use symphony::event_stream::EventHub;
use symphony::http_server::{
    build_router, EventStreamConfig, HttpServerState, RefreshControl, SnapshotSource,
};
use symphony::orchestrator::Orchestrator;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;

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
struct NoopRefreshControl;

impl RefreshControl for NoopRefreshControl {
    fn request_refresh(&self) -> symphony::domain::RefreshRequestOutcome {
        symphony::domain::RefreshRequestOutcome {
            queued: true,
            coalesced: false,
            pending_requests: 1,
        }
    }
}

fn fixture_snapshot() -> OrchestratorSnapshot {
    let started_at = Utc::now();

    OrchestratorSnapshot {
        poll_interval_ms: 30_000,
        max_concurrent_agents: 4,
        tracker_project_url: Some("https://linear.app/kata-sh/project/symphony".to_string()),
        running: BTreeMap::from([(
            "issue-920".to_string(),
            RunAttempt {
                issue_id: "issue-920".to_string(),
                issue_identifier: "KAT-920".to_string(),
                issue_title: Some("Worker issue".to_string()),
                attempt: Some(1),
                workspace_path: "/tmp/symphony/issue-920".to_string(),
                started_at,
                status: "running".to_string(),
                error: None,
                worker_host: None,
                model: None,
                tracker_state: Some("In Progress".to_string()),
                issue_url: None,
            },
        )]),
        running_sessions: BTreeMap::from([(
            "issue-920".to_string(),
            RunningSessionSnapshot {
                turn_count: 1,
                last_activity_at: Some(started_at),
                total_tokens: 0,
                last_event: Some("session_started".to_string()),
                last_event_message: None,
                session_id: Some("session-920".to_string()),
                current_tool_name: None,
                current_tool_args_preview: None,
                last_error: None,
            },
        )]),
        running_session_info: BTreeMap::from([(
            "issue-920".to_string(),
            WorkerSessionInfo {
                turn_count: 1,
                max_turns: 20,
                stall_timeout_ms: 300_000,
                last_activity_ms: Some(started_at.timestamp_millis()),
                session_tokens: Default::default(),
                current_tool_name: None,
                current_tool_args_preview: None,
                last_error: None,
            },
        )]),
        claimed: BTreeSet::from(["issue-920".to_string()]),
        retry_queue: vec![RetrySnapshotEntry {
            issue_id: "issue-777".to_string(),
            identifier: "KAT-777".to_string(),
            attempt: 2,
            due_in_ms: 1000,
            error: Some("retry".to_string()),
            worker_host: None,
            workspace_path: None,
        }],
        completed: vec![CompletedEntry {
            issue_id: "issue-001".to_string(),
            identifier: "KAT-1".to_string(),
            title: "done".to_string(),
            completed_at: Some(Utc::now()),
        }],
        blocked: vec![],
        pending_escalations: vec![],
        shared_context: symphony::domain::SharedContextSummary::default(),
        supervisor: symphony::domain::SupervisorSnapshot::default(),
        codex_totals: CodexTotals::default(),
        codex_rate_limits: None,
        polling: PollingSnapshot {
            checking: false,
            next_poll_in_ms: 500,
            poll_interval_ms: 30_000,
            last_poll_at: Some(Utc::now().to_rfc3339()),
            poll_count: 1,
        },
        triage_sessions: vec![],
    }
}

async fn spawn_server(
    config: EventStreamConfig,
) -> (
    std::net::SocketAddr,
    tokio::task::JoinHandle<()>,
    HttpServerState,
) {
    let event_hub = EventHub::new(128);
    let state = HttpServerState::with_event_stream(
        Arc::new(StaticSnapshotSource {
            snapshot: fixture_snapshot(),
        }),
        Arc::new(NoopRefreshControl),
        symphony::orchestrator::EscalationRegistry::default(),
        event_hub,
        config,
    );

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener
        .local_addr()
        .expect("listener should expose local addr");
    let app = build_router(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("test server should run");
    });

    (addr, task, state)
}

async fn recv_envelope(
    stream: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    timeout: Duration,
) -> SymphonyEventEnvelope {
    let message = tokio::time::timeout(timeout, stream.next())
        .await
        .expect("websocket read should not time out")
        .expect("stream should produce websocket frame")
        .expect("frame should be readable");

    let text = message.into_text().expect("frame should be text");
    serde_json::from_str(&text).expect("text frame should be valid event envelope json")
}

#[tokio::test]
async fn websocket_upgrade_and_heartbeat() {
    let config = EventStreamConfig {
        heartbeat_interval: Duration::from_millis(100),
        ..EventStreamConfig::default()
    };
    let (addr, server_task, _state) = spawn_server(config).await;

    let url = format!("ws://{addr}/api/v1/events");
    let (mut stream, _) = connect_async(url).await.expect("websocket should connect");

    let snapshot = recv_envelope(&mut stream, Duration::from_secs(2)).await;
    assert_eq!(snapshot.kind, EventKind::Snapshot);
    assert_eq!(snapshot.event, "snapshot");

    let heartbeat = loop {
        let envelope = recv_envelope(&mut stream, Duration::from_secs(2)).await;
        if envelope.kind == EventKind::Heartbeat {
            break envelope;
        }
    };

    assert_eq!(heartbeat.event, "heartbeat");

    server_task.abort();
}

#[tokio::test]
async fn websocket_ping_receives_protocol_pong() {
    let config = EventStreamConfig {
        heartbeat_interval: Duration::from_secs(5),
        ..EventStreamConfig::default()
    };
    let (addr, server_task, _state) = spawn_server(config).await;

    let url = format!("ws://{addr}/api/v1/events");
    let (mut stream, _) = connect_async(url).await.expect("websocket should connect");

    let snapshot = recv_envelope(&mut stream, Duration::from_secs(2)).await;
    assert_eq!(snapshot.kind, EventKind::Snapshot);

    let ping_payload = vec![1_u8, 2, 3, 4];
    stream
        .send(tokio_tungstenite::tungstenite::Message::Ping(
            ping_payload.clone().into(),
        ))
        .await
        .expect("client ping should send");

    let pong = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match stream.next().await {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Pong(payload))) => {
                    break payload;
                }
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                    let envelope: SymphonyEventEnvelope = serde_json::from_str(&text)
                        .expect("text websocket frame should decode as event envelope");
                    if envelope.kind != EventKind::Heartbeat {
                        panic!(
                            "unexpected non-heartbeat text frame while waiting for pong: {text}"
                        );
                    }
                }
                Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(_))) => {
                    panic!("unexpected binary frame while waiting for pong");
                }
                Some(Ok(_)) => {}
                Some(Err(err)) => panic!("unexpected websocket error: {err}"),
                None => panic!("websocket closed before pong"),
            }
        }
    })
    .await
    .expect("server should respond to ping with pong");

    assert_eq!(pong.to_vec(), ping_payload);

    let quiet_until = tokio::time::Instant::now() + Duration::from_millis(200);
    loop {
        let now = tokio::time::Instant::now();
        if now >= quiet_until {
            break;
        }

        let remaining = quiet_until - now;
        match tokio::time::timeout(remaining, stream.next()).await {
            Err(_) => break,
            Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text)))) => {
                let envelope: SymphonyEventEnvelope = serde_json::from_str(&text)
                    .expect("text websocket frame should decode as event envelope");
                if envelope.kind != EventKind::Heartbeat {
                    panic!("unexpected non-heartbeat text frame after pong: {text}");
                }
            }
            Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(_)))) => {
                panic!("unexpected binary application frame after pong");
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(err))) => panic!("unexpected websocket error during quiet period: {err}"),
            Ok(None) => break,
        }
    }

    server_task.abort();
}

#[tokio::test]
async fn filtered_runtime_event_delivery() {
    let config = EventStreamConfig {
        heartbeat_interval: Duration::from_secs(5),
        ..EventStreamConfig::default()
    };
    let (addr, server_task, state) = spawn_server(config).await;

    let url = format!("ws://{addr}/api/v1/events?issue=KAT-920&type=tool&severity=info");
    let (mut stream, _) = connect_async(url).await.expect("websocket should connect");

    let snapshot = recv_envelope(&mut stream, Duration::from_secs(2)).await;
    assert_eq!(snapshot.kind, EventKind::Snapshot);

    let mut orchestrator = Orchestrator::new(Default::default(), "prompt".to_string());
    orchestrator.attach_event_hub(state.event_hub());
    orchestrator.state_mut().running.insert(
        "issue-920".to_string(),
        RunAttempt {
            issue_id: "issue-920".to_string(),
            issue_identifier: "KAT-920".to_string(),
            issue_title: Some("Worker issue".to_string()),
            attempt: Some(1),
            workspace_path: "/tmp/symphony/issue-920".to_string(),
            started_at: Utc::now(),
            status: "running".to_string(),
            error: None,
            worker_host: None,
            model: None,
            tracker_state: Some("In Progress".to_string()),
            issue_url: None,
        },
    );

    orchestrator.ingest_agent_event(
        "issue-920",
        &AgentEvent::ToolCallCompleted {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            tool_name: "bash".to_string(),
        },
    );

    let envelope = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let envelope = recv_envelope(&mut stream, Duration::from_secs(2)).await;
            if envelope.kind == EventKind::Tool {
                break envelope;
            }
        }
    })
    .await
    .expect("expected a tool envelope before timeout");

    assert_eq!(envelope.severity, EventSeverity::Info);
    assert_eq!(envelope.issue.as_deref(), Some("KAT-920"));
    assert_eq!(envelope.event, "tool_call_completed");

    server_task.abort();
}

#[tokio::test]
async fn orchestrator_event_stream_emits_runtime_and_tool_events() {
    let hub = EventHub::new(64);
    let mut receiver = hub.subscribe();

    let mut orchestrator = Orchestrator::new(Default::default(), "prompt".to_string());
    orchestrator.attach_event_hub(hub.clone());
    orchestrator.state_mut().running.insert(
        "issue-920".to_string(),
        RunAttempt {
            issue_id: "issue-920".to_string(),
            issue_identifier: "KAT-920".to_string(),
            issue_title: Some("Worker issue".to_string()),
            attempt: Some(1),
            workspace_path: "/tmp/symphony/issue-920".to_string(),
            started_at: Utc::now(),
            status: "running".to_string(),
            error: None,
            worker_host: None,
            model: None,
            tracker_state: Some("In Progress".to_string()),
            issue_url: None,
        },
    );

    orchestrator.ingest_agent_event(
        "issue-920",
        &AgentEvent::ToolCallCompleted {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            tool_name: "bash".to_string(),
        },
    );

    orchestrator.handle_worker_completion(
        "issue-920",
        symphony::orchestrator::WorkerCompletion::Completed {
            schedule_continuation: false,
        },
        Utc::now().timestamp_millis(),
    );

    let first = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("first event should arrive")
        .expect("first event should decode");
    let second = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("second event should arrive")
        .expect("second event should decode");

    assert_eq!(first.kind, EventKind::Tool);
    assert_eq!(first.issue.as_deref(), Some("KAT-920"));
    assert_eq!(second.kind, EventKind::Worker);
    assert_eq!(second.event, "worker_completed");
}

#[tokio::test]
async fn orchestrator_session_started_event_includes_session_id() {
    let hub = EventHub::new(64);
    let mut receiver = hub.subscribe();

    let mut orchestrator = Orchestrator::new(Default::default(), "prompt".to_string());
    orchestrator.attach_event_hub(hub.clone());
    orchestrator.state_mut().running.insert(
        "issue-920".to_string(),
        RunAttempt {
            issue_id: "issue-920".to_string(),
            issue_identifier: "KAT-920".to_string(),
            issue_title: Some("Worker issue".to_string()),
            attempt: Some(1),
            workspace_path: "/tmp/symphony/issue-920".to_string(),
            started_at: Utc::now(),
            status: "running".to_string(),
            error: None,
            worker_host: None,
            model: None,
            tracker_state: Some("In Progress".to_string()),
            issue_url: None,
        },
    );

    orchestrator.ingest_agent_event(
        "issue-920",
        &AgentEvent::SessionStarted {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            session_id: "session-920".to_string(),
        },
    );

    let envelope = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("session_started envelope should arrive")
        .expect("session_started envelope should decode");

    assert_eq!(envelope.event, "session_started");
    assert_eq!(envelope.issue.as_deref(), Some("KAT-920"));
    assert_eq!(
        envelope
            .payload
            .get("session_id")
            .and_then(|value| value.as_str()),
        Some("session-920")
    );
}

#[tokio::test]
async fn tool_error_notification_maps_to_tool_error_envelope() {
    let hub = EventHub::new(64);
    let mut receiver = hub.subscribe();

    let mut orchestrator = Orchestrator::new(Default::default(), "prompt".to_string());
    orchestrator.attach_event_hub(hub.clone());
    orchestrator.state_mut().running.insert(
        "issue-920".to_string(),
        RunAttempt {
            issue_id: "issue-920".to_string(),
            issue_identifier: "KAT-920".to_string(),
            issue_title: Some("Worker issue".to_string()),
            attempt: Some(1),
            workspace_path: "/tmp/symphony/issue-920".to_string(),
            started_at: Utc::now(),
            status: "running".to_string(),
            error: None,
            worker_host: None,
            model: None,
            tracker_state: Some("In Progress".to_string()),
            issue_url: None,
        },
    );

    orchestrator.ingest_agent_event(
        "issue-920",
        &AgentEvent::Notification {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            message: "tool_error: bash".to_string(),
        },
    );

    let envelope = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("tool_error envelope should arrive")
        .expect("tool_error envelope should decode");

    assert_eq!(envelope.kind, EventKind::Tool);
    assert_eq!(envelope.severity, EventSeverity::Error);
    assert_eq!(envelope.event, "tool_error");
    assert_eq!(
        envelope
            .payload
            .get("summary")
            .and_then(|value| value.as_str()),
        Some("bash")
    );
}

#[tokio::test]
async fn tool_error_envelope_includes_last_tool_command_preview() {
    let hub = EventHub::new(64);
    let mut receiver = hub.subscribe();

    let mut orchestrator = Orchestrator::new(Default::default(), "prompt".to_string());
    orchestrator.attach_event_hub(hub.clone());
    orchestrator.state_mut().running.insert(
        "issue-921".to_string(),
        RunAttempt {
            issue_id: "issue-921".to_string(),
            issue_identifier: "KAT-921".to_string(),
            issue_title: Some("Worker issue".to_string()),
            attempt: Some(1),
            workspace_path: "/tmp/symphony/issue-921".to_string(),
            started_at: Utc::now(),
            status: "running".to_string(),
            error: None,
            worker_host: None,
            model: None,
            tracker_state: Some("In Progress".to_string()),
            issue_url: None,
        },
    );

    orchestrator.ingest_agent_event(
        "issue-921",
        &AgentEvent::Notification {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            message:
                r#"tool_start: bash {"command":"cd apps/symphony && pnpm test","timeout":1200}"#
                    .to_string(),
        },
    );
    let _ = receiver.recv().await.expect("tool_start envelope");

    orchestrator.ingest_agent_event(
        "issue-921",
        &AgentEvent::Notification {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            message: "tool_error: bash".to_string(),
        },
    );

    let envelope = tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("tool_error envelope should arrive")
        .expect("tool_error envelope should decode");

    assert_eq!(envelope.event, "tool_error");
    assert_eq!(
        envelope
            .payload
            .get("error_preview")
            .and_then(|value| value.as_str()),
        Some("bash: cd apps/symphony && pnpm test")
    );
}

#[tokio::test]
async fn slow_consumer_backpressure() {
    let config = EventStreamConfig {
        heartbeat_interval: Duration::from_secs(30),
        client_queue_capacity: 1,
        backpressure_drop_threshold: 1,
        writer_send_delay: Some(Duration::from_millis(200)),
    };
    let (addr, server_task, state) = spawn_server(config).await;

    let url = format!("ws://{addr}/api/v1/events");
    let (mut stream, _) = connect_async(url).await.expect("websocket should connect");

    let _snapshot = recv_envelope(&mut stream, Duration::from_secs(2)).await;

    for idx in 0..10 {
        state.event_hub().publish(
            EventKind::Worker,
            EventSeverity::Info,
            Some("KAT-920".to_string()),
            "worker_update",
            json!({ "idx": idx }),
        );
    }

    let close_reason = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            match stream.next().await {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(frame))) => {
                    break frame
                        .map(|frame| frame.reason.to_string())
                        .unwrap_or_else(|| "closed".to_string());
                }
                Some(Ok(_)) => {}
                Some(Err(err)) => break format!("error:{err}"),
                None => break "none".to_string(),
            }
        }
    })
    .await
    .expect("stream should close with backpressure");

    assert_eq!(close_reason, "backpressure");

    let counters = state.event_stream_counters();
    assert!(
        counters.dropped >= 1,
        "expected dropped counter to increment"
    );

    server_task.abort();
}

#[tokio::test]
async fn queue_full_honors_backpressure_drop_threshold() {
    let config = EventStreamConfig {
        heartbeat_interval: Duration::from_secs(30),
        client_queue_capacity: 1,
        backpressure_drop_threshold: 3,
        writer_send_delay: Some(Duration::from_millis(250)),
    };
    let (addr, server_task, state) = spawn_server(config).await;

    let url = format!("ws://{addr}/api/v1/events");
    let (mut stream, _) = connect_async(url).await.expect("websocket should connect");

    let _snapshot = recv_envelope(&mut stream, Duration::from_secs(2)).await;

    for idx in 0..24 {
        state.event_hub().publish(
            EventKind::Worker,
            EventSeverity::Info,
            Some("KAT-920".to_string()),
            "worker_update",
            json!({ "idx": idx }),
        );
    }

    let close_reason = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            match stream.next().await {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(frame))) => {
                    break frame
                        .map(|frame| frame.reason.to_string())
                        .unwrap_or_else(|| "closed".to_string());
                }
                Some(Ok(_)) => {}
                Some(Err(err)) => break format!("error:{err}"),
                None => break "none".to_string(),
            }
        }
    })
    .await
    .expect("stream should close after reaching configured drop threshold");

    assert_eq!(close_reason, "backpressure");

    let counters = state.event_stream_counters();
    assert!(
        counters.dropped >= 3,
        "expected dropped counter to honor configured threshold before disconnect"
    );

    server_task.abort();
}
