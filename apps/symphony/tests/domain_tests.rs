use chrono::Utc;
use std::collections::{BTreeMap, BTreeSet};
use symphony::domain::*;
use symphony::error::SymphonyError;

// ── Issue round-trip (T02) ─────────────────────────────────────────────

#[test]
fn test_issue_json_round_trip() {
    let issue = Issue {
        id: "issue-42".into(),
        identifier: "PROJ-42".into(),
        title: "Fix the widget".into(),
        description: Some("Detailed description here".into()),
        priority: Some(2),
        state: "In Progress".into(),
        branch_name: Some("feat/widget-fix".into()),
        url: Some("https://linear.app/proj/issue/PROJ-42".into()),
        assignee_id: Some("user-1".into()),
        labels: vec!["bug".into(), "urgent".into()],
        blocked_by: vec![BlockerRef {
            id: Some("issue-10".into()),
            identifier: Some("PROJ-10".into()),
            state: Some("Done".into()),
        }],
        assigned_to_worker: true,
        created_at: Some(Utc::now()),
        updated_at: Some(Utc::now()),
        children_count: 0,
        parent_identifier: None,
    };

    let json = serde_json::to_string(&issue).unwrap();
    let deser: Issue = serde_json::from_str(&json).unwrap();

    assert_eq!(deser.id, "issue-42");
    assert_eq!(deser.identifier, "PROJ-42");
    assert_eq!(deser.title, "Fix the widget");
    assert_eq!(
        deser.description.as_deref(),
        Some("Detailed description here")
    );
    assert_eq!(deser.priority, Some(2));
    assert_eq!(deser.state, "In Progress");
    assert_eq!(deser.branch_name.as_deref(), Some("feat/widget-fix"));
    assert_eq!(
        deser.url.as_deref(),
        Some("https://linear.app/proj/issue/PROJ-42")
    );
    assert_eq!(deser.assignee_id.as_deref(), Some("user-1"));
    assert_eq!(deser.labels, vec!["bug", "urgent"]);
    assert_eq!(deser.blocked_by.len(), 1);
    assert_eq!(deser.blocked_by[0].identifier.as_deref(), Some("PROJ-10"));
    assert!(deser.assigned_to_worker);
    assert!(deser.created_at.is_some());
    assert!(deser.updated_at.is_some());
}

// ── Issue missing optionals defaults (T02) ─────────────────────────────

#[test]
fn test_issue_missing_optionals_defaults() {
    let json = r#"{
        "id": "1",
        "identifier": "X-1",
        "title": "t",
        "state": "Todo",
        "assigned_to_worker": true
    }"#;
    let issue: Issue = serde_json::from_str(json).unwrap();

    assert_eq!(issue.id, "1");
    assert_eq!(issue.identifier, "X-1");
    assert_eq!(issue.title, "t");
    assert_eq!(issue.state, "Todo");
    assert!(issue.assigned_to_worker);
    // All optional/defaulted fields
    assert!(issue.description.is_none());
    assert!(issue.priority.is_none());
    assert!(issue.branch_name.is_none());
    assert!(issue.url.is_none());
    assert!(issue.assignee_id.is_none());
    assert!(issue.labels.is_empty());
    assert!(issue.blocked_by.is_empty());
    assert!(issue.created_at.is_none());
    assert!(issue.updated_at.is_none());
}

// ── Issue assigned_to_worker defaults to true (T02) ────────────────────

#[test]
fn test_issue_assigned_to_worker_defaults_true() {
    // When assigned_to_worker is missing entirely, serde default_true kicks in
    let json = r#"{
        "id": "2",
        "identifier": "X-2",
        "title": "t2",
        "state": "Todo"
    }"#;
    let issue: Issue = serde_json::from_str(json).unwrap();
    assert!(issue.assigned_to_worker);
}

// ── ServiceConfig defaults match spec §5.3 (T02) ──────────────────────

#[test]
fn test_service_config_defaults_match_spec() {
    let cfg = ServiceConfig {
        tracker: TrackerConfig::default(),
        polling: PollingConfig::default(),
        workspace: WorkspaceConfig::default(),
        worker: WorkerConfig::default(),
        agent: AgentConfig::default(),
        codex: CodexConfig::default(),
        pi_agent: PiAgentConfig::default(),
        agent_backend: AgentBackend::default(),
        hooks: HooksConfig::default(),
        server: ServerConfig::default(),
        prompts: None,
        notifications: None,
        shared_context: SharedContextConfig::default(),
        supervisor: SupervisorConfig::default(),
        storage: symphony::triage::domain::StorageConfig::default(),
        triage: symphony::triage::domain::TriageConfig::default(),
    };

    // Polling §5.3.2
    assert_eq!(cfg.polling.interval_ms, 30_000);

    // Agent §5.3.5
    assert_eq!(cfg.agent.max_concurrent_agents, 10);
    assert_eq!(cfg.agent.max_turns, 20);
    assert_eq!(cfg.agent.max_retry_backoff_ms, 300_000);
    assert_eq!(cfg.agent.escalation_timeout_ms, 300_000);

    // Codex §5.3.6
    assert_eq!(cfg.codex.turn_timeout_ms, 3_600_000);
    assert_eq!(cfg.codex.read_timeout_ms, 5_000);
    assert_eq!(cfg.codex.stall_timeout_ms, 300_000);
    assert_eq!(cfg.pi_agent.read_timeout_ms, 5_000);
    assert_eq!(cfg.pi_agent.stall_timeout_ms, 300_000);
    assert_eq!(cfg.agent_backend, AgentBackend::KataCli);

    // Hooks §5.3.4
    assert_eq!(cfg.hooks.timeout_ms, 60_000);

    // Server §13.7
    assert_eq!(cfg.server.host, "127.0.0.1");
    assert!(cfg.server.public_url.is_none());

    // Tracker §5.3.1
    assert_eq!(cfg.tracker.endpoint, "https://api.linear.app/graphql");

    // Shared context defaults
    assert_eq!(cfg.shared_context.ttl_ms, 86_400_000);
    assert_eq!(cfg.shared_context.max_entries, 100);

    // Supervisor defaults
    assert!(!cfg.supervisor.enabled);
    assert!(cfg.supervisor.model.is_none());
    assert_eq!(cfg.supervisor.steer_cooldown_ms, 120_000);

    // Storage / triage A1 defaults
    assert_eq!(cfg.storage.path, None);
    assert_eq!(cfg.storage.busy_timeout_ms, 5_000);
    assert!(!cfg.triage.enabled);
    assert_eq!(
        cfg.triage.mode,
        symphony::triage::domain::TriageMode::Preview
    );
    assert_eq!(cfg.triage.intake_label, "needs-triage");
    assert_eq!(cfg.triage.prompt, "prompts/triage.md");
    assert_eq!(cfg.triage.turn_timeout_ms, 900_000);
    assert_eq!(cfg.triage.max_attempts, 3);
    assert_eq!(cfg.triage.max_intake_pages, 100);
}

#[test]
fn test_tracker_project_url_uses_linear_project_and_workspace_slug() {
    let mut tracker = TrackerConfig::default();
    tracker.project_slug = Some("symphony".to_string());
    assert_eq!(
        tracker.tracker_project_url().as_deref(),
        Some("https://linear.app/kata-sh/project/symphony")
    );

    tracker.workspace_slug = Some("acme".to_string());
    assert_eq!(
        tracker.tracker_project_url().as_deref(),
        Some("https://linear.app/acme/project/symphony")
    );

    tracker.project_slug = None;
    assert_eq!(tracker.tracker_project_url(), None);
}

#[test]
fn test_tracker_project_url_uses_github_project_path_for_github_kind() {
    let tracker = TrackerConfig {
        kind: Some("github".to_string()),
        repo_owner: Some("kata-sh".to_string()),
        repo_name: Some("kata-mono".to_string()),
        github_project_owner_type: Some(GithubProjectOwnerType::User),
        github_project_number: Some(17),
        ..TrackerConfig::default()
    };

    assert_eq!(
        tracker.tracker_project_url().as_deref(),
        Some("https://github.com/users/kata-sh/projects/17")
    );
}

#[test]
fn test_tracker_project_url_uses_github_org_project_path_for_github_kind() {
    let tracker = TrackerConfig {
        kind: Some("github".to_string()),
        repo_owner: Some("kata-sh".to_string()),
        repo_name: Some("kata-mono".to_string()),
        github_project_owner_type: Some(GithubProjectOwnerType::Org),
        github_project_number: Some(17),
        ..TrackerConfig::default()
    };

    assert_eq!(
        tracker.tracker_project_url().as_deref(),
        Some("https://github.com/orgs/kata-sh/projects/17")
    );
}

// ── ServerConfig default fix (T01 must-have) ───────────────────────────

#[test]
fn test_server_config_default_host() {
    let cfg = ServerConfig::default();
    assert_eq!(cfg.host, "127.0.0.1");
    assert!(cfg.port.is_none());
    assert!(cfg.public_url.is_none());
}

// ── Runtime entity construction (T01+T02) ──────────────────────────────

#[test]
fn test_run_attempt_construction_and_serialization() {
    let ra = RunAttempt {
        issue_id: "id-1".into(),
        issue_identifier: "PROJ-1".into(),
        issue_title: None,
        attempt: None,
        workspace_path: "/tmp/ws".into(),
        started_at: Utc::now(),
        status: "running".into(),
        error: None,
        worker_host: None,
        model: None,
        tracker_state: None,
        issue_url: None,
    };
    let json = serde_json::to_string(&ra).unwrap();
    let deser: RunAttempt = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.issue_id, "id-1");
    assert!(deser.attempt.is_none());
}

#[test]
fn test_run_attempt_deserializes_when_issue_url_missing() {
    let json = r#"{
        "issue_id":"id-legacy",
        "issue_identifier":"PROJ-LEGACY",
        "issue_title":null,
        "attempt":null,
        "workspace_path":"/tmp/ws",
        "started_at":"2026-01-01T00:00:00Z",
        "status":"running",
        "error":null,
        "worker_host":null,
        "model":null,
        "tracker_state":null
    }"#;
    let deser: RunAttempt = serde_json::from_str(json).unwrap();
    assert_eq!(deser.issue_id, "id-legacy");
    assert_eq!(deser.issue_url, None);
}

#[test]
fn test_live_session_token_defaults() {
    let json = r#"{
        "session_id": "s1",
        "thread_id": "t1",
        "turn_id": "turn1",
        "started_at": "2026-01-01T00:00:00Z"
    }"#;
    let ls: LiveSession = serde_json::from_str(json).unwrap();
    assert_eq!(ls.codex_input_tokens, 0);
    assert_eq!(ls.codex_output_tokens, 0);
    assert_eq!(ls.codex_total_tokens, 0);
    assert_eq!(ls.last_reported_input_tokens, 0);
    assert_eq!(ls.last_reported_output_tokens, 0);
    assert_eq!(ls.last_reported_total_tokens, 0);
    assert_eq!(ls.turn_count, 0);
}

#[test]
fn test_retry_entry_construction() {
    let re = RetryEntry {
        issue_id: "id-2".into(),
        identifier: "PROJ-2".into(),
        attempt: 1,
        due_at_ms: 1234567890,
        timer_handle: None,
        error: Some("timeout".into()),
        worker_host: None,
        workspace_path: Some("/tmp/ws2".into()),
    };
    assert_eq!(re.attempt, 1);
    assert_eq!(re.error.as_deref(), Some("timeout"));
}

#[test]
fn test_workspace_construction() {
    let ws = Workspace {
        path: "/tmp/symphony_workspaces/PROJ-42".into(),
        workspace_key: "PROJ-42".into(),
        created_now: true,
    };
    assert_eq!(ws.workspace_key, "PROJ-42");
    assert!(ws.created_now);
    assert!(ws.path.contains("PROJ-42"));
}

// ── Snapshot types (T01 must-have: BTreeMap for deterministic JSON) ────

#[test]
fn test_orchestrator_snapshot_serializes() {
    let snap = OrchestratorSnapshot {
        poll_interval_ms: 30_000,
        max_concurrent_agents: 5,
        tracker_project_url: Some("https://linear.app/kata-sh/project/symphony".to_string()),
        running: {
            let mut m = BTreeMap::new();
            m.insert(
                "z-issue".to_string(),
                RunAttempt {
                    issue_id: "id-z".into(),
                    issue_identifier: "PROJ-Z".into(),
                    issue_title: None,
                    attempt: Some(1),
                    workspace_path: "/tmp/ws-z".into(),
                    started_at: Utc::now(),
                    status: "running".into(),
                    error: None,
                    worker_host: None,
                    model: None,
                    tracker_state: None,
                    issue_url: None,
                },
            );
            m.insert(
                "a-issue".to_string(),
                RunAttempt {
                    issue_id: "id-a".into(),
                    issue_identifier: "PROJ-A".into(),
                    issue_title: None,
                    attempt: Some(1),
                    workspace_path: "/tmp/ws-a".into(),
                    started_at: Utc::now(),
                    status: "running".into(),
                    error: None,
                    worker_host: None,
                    model: None,
                    tracker_state: None,
                    issue_url: None,
                },
            );
            m
        },
        running_sessions: {
            let mut m = BTreeMap::new();
            m.insert(
                "a-issue".to_string(),
                RunningSessionSnapshot {
                    turn_count: 3,
                    last_activity_at: Some(Utc::now()),
                    total_tokens: 120,
                    last_event: None,
                    last_event_message: None,
                    session_id: None,
                    current_tool_name: None,
                    current_tool_args_preview: None,
                    last_error: None,
                },
            );
            m.insert(
                "z-issue".to_string(),
                RunningSessionSnapshot {
                    turn_count: 1,
                    last_activity_at: Some(Utc::now()),
                    total_tokens: 40,
                    last_event: None,
                    last_event_message: None,
                    session_id: None,
                    current_tool_name: None,
                    current_tool_args_preview: None,
                    last_error: None,
                },
            );
            m
        },
        running_session_info: BTreeMap::new(),
        claimed: {
            let mut s = BTreeSet::new();
            s.insert("c-claim".to_string());
            s.insert("a-claim".to_string());
            s
        },
        retry_queue: vec![RetrySnapshotEntry {
            issue_id: "id-5".into(),
            identifier: "PROJ-5".into(),
            attempt: 2,
            due_in_ms: 5000,
            error: Some("rate limited".into()),
            worker_host: None,
            workspace_path: Some("/tmp/ws5".into()),
        }],
        completed: vec![
            CompletedEntry {
                issue_id: "z-done".to_string(),
                identifier: "KAT-100".to_string(),
                title: "Done issue Z".to_string(),
                completed_at: Some(Utc::now()),
            },
            CompletedEntry {
                issue_id: "a-done".to_string(),
                identifier: "KAT-101".to_string(),
                title: "Done issue A".to_string(),
                completed_at: Some(Utc::now()),
            },
        ],
        blocked: vec![],
        pending_escalations: vec![],
        shared_context: symphony::domain::SharedContextSummary::default(),
        supervisor: symphony::domain::SupervisorSnapshot::default(),
        codex_totals: CodexTotals::default(),
        codex_rate_limits: None,
        polling: PollingSnapshot {
            checking: false,
            next_poll_in_ms: 15_000,
            poll_interval_ms: 30_000,
            last_poll_at: None,
            poll_count: 0,
        },
        triage_sessions: vec![],
    };
    let json = serde_json::to_string(&snap).unwrap();
    // Valid JSON
    let val: serde_json::Value = serde_json::from_str(&json).unwrap();
    // Contains expected keys
    assert!(val.get("poll_interval_ms").is_some());
    assert!(val.get("max_concurrent_agents").is_some());
    assert_eq!(
        val.get("tracker_project_url").and_then(|v| v.as_str()),
        Some("https://linear.app/kata-sh/project/symphony")
    );
    assert!(val.get("running").is_some());
    assert!(val.get("running_sessions").is_some());
    assert!(val.get("running_session_info").is_some());
    assert!(val.get("pending_escalations").is_some());
    assert!(val.get("retry_queue").is_some());
    assert!(val.get("completed").is_some());
    assert!(val.get("shared_context").is_some());
    assert!(val.get("codex_totals").is_some());
    assert!(val.get("polling").is_some());
    // Retry queue has our entry
    let queue = val["retry_queue"].as_array().unwrap();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0]["identifier"], "PROJ-5");

    // BTreeMap running keys serialize in sorted order
    let running_start = json.find("\"running\":{").unwrap();
    let running_json = &json[running_start..];
    let a_pos = running_json.find("\"a-issue\"").unwrap();
    let z_pos = running_json.find("\"z-issue\"").unwrap();
    assert!(
        a_pos < z_pos,
        "running keys should serialize in sorted order"
    );

    // BTreeSet claimed serializes in sorted order
    let claimed_start = json.find("\"claimed\":[").unwrap();
    let claimed_json = &json[claimed_start..];
    let a_pos = claimed_json.find("\"a-claim\"").unwrap();
    let c_pos = claimed_json.find("\"c-claim\"").unwrap();
    assert!(a_pos < c_pos, "claimed should serialize in sorted order");

    // Completed entries serialize as objects with identifier and title
    let completed_start = json.find("\"completed\":[").unwrap();
    let completed_json = &json[completed_start..];
    assert!(
        completed_json.contains("\"identifier\":\"KAT-100\""),
        "completed should contain identifier field"
    );
    assert!(
        completed_json.contains("\"title\":\"Done issue Z\""),
        "completed should contain title field"
    );
}

#[test]
fn test_escalation_request_response_round_trip() {
    let request = EscalationRequest {
        id: "esc-100".to_string(),
        issue_id: "issue-100".to_string(),
        issue_identifier: "KAT-100".to_string(),
        method: "ask_user_questions".to_string(),
        payload: serde_json::json!({
            "questions": [
                {
                    "id": "approach",
                    "header": "Approach",
                    "question": "Choose implementation strategy",
                    "options": [
                        { "label": "A", "description": "Option A" }
                    ]
                }
            ]
        }),
        created_at: Utc::now(),
        timeout_ms: 300_000,
    };

    let encoded_request = serde_json::to_string(&request).unwrap();
    let decoded_request: EscalationRequest = serde_json::from_str(&encoded_request).unwrap();
    assert_eq!(decoded_request.id, "esc-100");
    assert_eq!(decoded_request.method, "ask_user_questions");

    let response = EscalationResponse {
        request_id: request.id.clone(),
        response: serde_json::json!({ "cancelled": false, "value": "A" }),
        responder_id: Some("operator-7".to_string()),
        responded_at: Utc::now(),
    };

    let encoded_response = serde_json::to_string(&response).unwrap();
    let decoded_response: EscalationResponse = serde_json::from_str(&encoded_response).unwrap();
    assert_eq!(decoded_response.request_id, "esc-100");
    assert_eq!(decoded_response.responder_id.as_deref(), Some("operator-7"));
}

// ── AgentEvent enum (T01+T02 must-have) ───────────────────────────────

#[test]
fn test_agent_event_variants() {
    let events: Vec<AgentEvent> = vec![
        AgentEvent::SessionStarted {
            timestamp: Utc::now(),
            codex_app_server_pid: Some("1234".into()),
            session_id: "sess-1".into(),
        },
        AgentEvent::StartupFailed {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            error: "port in use".into(),
        },
        AgentEvent::TurnCompleted {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            turn_id: "t1".into(),
            message: Some("done".into()),
            input_tokens: 10,
            output_tokens: 4,
            total_tokens: 14,
            rate_limits: None,
        },
        AgentEvent::TurnFailed {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            turn_id: "t1".into(),
            error: "crash".into(),
        },
        AgentEvent::TurnCancelled {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            turn_id: "t1".into(),
        },
        AgentEvent::TurnEndedWithError {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            turn_id: "t1".into(),
            error: "oops".into(),
        },
        AgentEvent::TurnInputRequired {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            turn_id: "t1".into(),
            prompt: None,
        },
        AgentEvent::ApprovalAutoApproved {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            tool_call: "shell".into(),
        },
        AgentEvent::UnsupportedToolCall {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            tool_name: "unknown_tool".into(),
        },
        AgentEvent::EscalationCreated {
            timestamp: Utc::now(),
            issue_id: "issue-1".into(),
            issue_identifier: "KAT-1".into(),
            request: EscalationRequest {
                id: "esc-1".into(),
                issue_id: "issue-1".into(),
                issue_identifier: "KAT-1".into(),
                method: "ask_user_questions".into(),
                payload: serde_json::json!({"questions": []}),
                created_at: Utc::now(),
                timeout_ms: 300_000,
            },
        },
        AgentEvent::EscalationResponded {
            timestamp: Utc::now(),
            issue_id: "issue-1".into(),
            issue_identifier: "KAT-1".into(),
            request_id: "esc-1".into(),
            responder_id: Some("operator-1".into()),
            latency_ms: 42,
        },
        AgentEvent::EscalationTimedOut {
            timestamp: Utc::now(),
            issue_id: "issue-1".into(),
            issue_identifier: "KAT-1".into(),
            request_id: "esc-2".into(),
            timeout_ms: 60_000,
        },
        AgentEvent::EscalationCancelled {
            timestamp: Utc::now(),
            issue_id: "issue-1".into(),
            issue_identifier: "KAT-1".into(),
            request_id: "esc-3".into(),
            reason: "worker_exited".into(),
        },
        AgentEvent::Notification {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            message: "info".into(),
        },
        AgentEvent::OtherMessage {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            raw: serde_json::json!({"type": "unknown"}),
        },
        AgentEvent::Malformed {
            timestamp: Utc::now(),
            codex_app_server_pid: None,
            raw_text: "bad data".into(),
            parse_error: "expected json".into(),
        },
    ];
    assert_eq!(events.len(), 16);
    for event in &events {
        let debug = format!("{:?}", event);
        assert!(!debug.is_empty());
    }
}

// ── CodexTotals default ───────────────────────────────────────────────

#[test]
fn test_codex_totals_default_is_zero() {
    let t = CodexTotals::default();
    assert_eq!(t.input_tokens, 0);
    assert_eq!(t.output_tokens, 0);
    assert_eq!(t.total_tokens, 0);
    assert_eq!(t.seconds_running, 0.0);
}

// ── SymphonyError display (T02) ───────────────────────────────────────

#[test]
fn test_symphony_error_display() {
    // One variant from each of the 5 spec failure classes:
    // 1. Workflow/Config
    let e1 = SymphonyError::MissingWorkflowFile {
        path: "/tmp/workflow.md".into(),
        reason: "not found".into(),
    };
    let s1 = e1.to_string();
    assert!(!s1.is_empty());
    assert!(s1.contains("workflow"), "workflow error: {}", s1);

    // 2. Tracker
    let e2 = SymphonyError::LinearApiRequest("connection refused".into());
    let s2 = e2.to_string();
    assert!(!s2.is_empty());
    assert!(s2.contains("connection refused"), "tracker error: {}", s2);

    // 3. Workspace
    let e3 = SymphonyError::WorkspaceOutsideRoot {
        workspace: "/evil/path".into(),
        root: "/tmp/workspaces".into(),
    };
    let s3 = e3.to_string();
    assert!(!s3.is_empty());
    assert!(s3.contains("/evil/path"), "workspace error: {}", s3);

    // 4. Codex/Agent
    let e4 = SymphonyError::TurnTimeout;
    let s4 = e4.to_string();
    assert!(!s4.is_empty());
    assert!(s4.contains("timeout"), "codex error: {}", s4);

    // 5. Generic
    let e5 = SymphonyError::Other("something went wrong".into());
    let s5 = e5.to_string();
    assert!(!s5.is_empty());
    assert!(s5.contains("something went wrong"), "generic error: {}", s5);
}
