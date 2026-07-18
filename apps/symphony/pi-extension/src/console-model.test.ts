import { describe, expect, it } from "vitest";
import { buildEscalationRows, buildIssueRows, buildTriageRows, buildWorkerRows, formatEventRows } from "./console-model.ts";
import type { SymphonyEventEnvelope, SymphonyStateResponse } from "./http-client.ts";

function stateFixture(): SymphonyStateResponse {
  return {
    tracker_project_url: "https://linear.app/kata-sh/project/symphony",
    running: {
      "issue-123": {
        issue_id: "issue-123",
        issue_identifier: "SIM-123",
        issue_title: "Worker one",
        attempt: 2,
        workspace_path: "/tmp/symphony/issue-123",
        started_at: "2026-05-14T12:00:00Z",
        status: "running",
        worker_host: "worker-a",
        tracker_state: "In Progress",
      },
      "issue-777": {
        issue_id: "issue-777",
        issue_identifier: "SIM-777",
        issue_title: "Worker two",
        workspace_path: "/tmp/symphony/issue-777",
        started_at: "2026-05-14T12:05:00Z",
        status: "running",
        error: "agent exited after a very long error message that needs to be shortened for the console",
      },
    },
    running_sessions: {
      "issue-123": { turn_count: 2, last_activity_at: "2026-05-14T12:03:00Z", last_event: "tool_call_completed", last_event_message: "running cargo test" },
    },
    running_session_info: {
      "issue-123": { turn_count: 3, max_turns: 20, last_activity_ms: Date.parse("2026-05-14T12:04:00Z"), last_error: null },
      "issue-777": { turn_count: 1, max_turns: 10, last_activity_ms: null, last_error: "usage limit" },
    },
    retry_queue: [],
    blocked: [],
    completed: [],
    polling: { checking: false, next_poll_in_ms: 1000, poll_interval_ms: 30000 },
    shared_context: { total_entries: 0, entries_by_scope: {}, oldest_entry_at: null, newest_entry_at: null },
    supervisor: { active: true, steers_issued: 0, conflicts_detected: 0, patterns_detected: 0, escalations_created: 0 },
    codex_totals: { input_tokens: 0, output_tokens: 0, total_tokens: 0, event_count: 0, seconds_running: 0 },
    codex_rate_limits: null,
  };
}

describe("console model", () => {
  it("builds sorted worker rows with selected-worker detail fields", () => {
    const rows = buildWorkerRows(stateFixture());

    expect(rows.map((row) => row.issueIdentifier)).toEqual(["SIM-123", "SIM-777"]);
    expect(rows[0]).toMatchObject({
      issueId: "issue-123",
      issueIdentifier: "SIM-123",
      title: "Worker one",
      trackerState: "In Progress",
      attempt: "2",
      turnCount: "3",
      maxTurns: "20",
      lastActivity: "2026-05-14T12:04:00.000Z",
      workerHost: "worker-a",
      workspacePath: "/tmp/symphony/issue-123",
      errorPreview: "-",
    });
    expect(rows[1]?.attempt).toBe("1");
    expect(rows[1]?.trackerState).toBe("-");
    expect(rows[1]?.errorPreview).toBe("agent exited after a very long error message that needs to be shortened for the console");
  });

  it("builds issue rows across running, retry, blocked, and completed buckets", () => {
    const state = stateFixture();
    state.retry_queue = [
      {
        issue_id: "issue-retry",
        identifier: "SIM-200",
        attempt: 3,
        due_in_ms: 90000,
        error: "rate limit",
        worker_host: "host-b",
        workspace_path: "/tmp/retry",
      },
    ];
    state.blocked = [
      {
        issue_id: "issue-blocked",
        identifier: "SIM-300",
        title: "Blocked work",
        state: "Todo",
        blocker_identifiers: ["SIM-100", "SIM-101"],
      },
    ];
    state.completed = [
      {
        issue_id: "issue-done",
        identifier: "SIM-400",
        title: "Done work",
        completed_at: "2026-05-14T13:00:00Z",
      },
    ];

    const rows = buildIssueRows(state);

    expect(rows.map((row) => `${row.kind}:${row.issueIdentifier}`)).toEqual([
      "running:SIM-123",
      "running:SIM-777",
      "retry:SIM-200",
      "blocked:SIM-300",
      "completed:SIM-400",
    ]);
    expect(rows.find((row) => row.kind === "retry")).toMatchObject({
      title: "rate limit",
      status: "retry in 1m 30s",
      attempt: "3",
      workerHost: "host-b",
      workspacePath: "/tmp/retry",
      errorPreview: "rate limit",
    });
    expect(rows.find((row) => row.kind === "blocked")).toMatchObject({
      status: "Todo",
      blockers: "SIM-100, SIM-101",
    });
    expect(rows.find((row) => row.kind === "completed")).toMatchObject({
      status: "completed",
      completedAt: "2026-05-14T13:00:00Z",
    });
  });

  it("includes triage sessions between running and retry rows", () => {
    const state = stateFixture();
    state.triage_sessions = [
      {
        issue_identifier: "#1",
        run_id: "run-1",
        stage_run_id: "stage-1",
        attempt: 2,
        harness: "pi",
        model: "openai-codex/gpt-5.6-luna",
        started_at: "2026-05-14T12:00:00Z",
        last_activity_at: "2026-05-14T12:00:05Z",
        last_event: "tool_execution_start",
        last_event_message: "running read",
        current_tool_name: "read",
        current_tool_args_preview: "README.md",
        total_tokens: 12,
      },
    ];

    const issueRows = buildIssueRows(state);
    const triageRows = buildTriageRows(state);

    expect(issueRows.map((row) => `${row.kind}:${row.issueIdentifier}`)).toEqual([
      "running:SIM-123",
      "running:SIM-777",
      "triage:#1",
    ]);
    expect(triageRows).toEqual([
      {
        issueIdentifier: "#1",
        stageRunId: "stage-1",
        attempt: "2",
        harness: "pi",
        model: "openai-codex/gpt-5.6-luna",
        status: "triage#2",
        lastEvent: "tool_execution_start",
        message: "read: README.md",
        lastActivity: "2026-05-14T12:00:05Z",
        tokens: "12",
      },
    ]);
  });

  it("rounds retry wait times up to the next second", () => {
    const state = stateFixture();
    state.retry_queue = [
      {
        issue_id: "issue-retry",
        identifier: "SIM-200",
        attempt: 1,
        due_in_ms: 999,
      },
    ];

    const retryRow = buildIssueRows(state).find((row) => row.kind === "retry");

    expect(retryRow).toMatchObject({ status: "retry in 1s" });
  });

  it("preserves pending retry state when no retry error is present", () => {
    const state = stateFixture();
    state.retry_queue = [
      {
        issue_id: "issue-retry",
        identifier: "SIM-200",
        attempt: 1,
        due_in_ms: 1000,
      },
    ];

    const retryRow = buildIssueRows(state).find((row) => row.kind === "retry");

    expect(retryRow).toMatchObject({
      title: "pending retry",
      trackerState: "retry",
    });
  });

  it("builds escalation rows sorted by creation time and request id", () => {
    const state = stateFixture();
    state.pending_escalations = [
      {
        request_id: "esc-3",
        issue_id: "issue-456",
        issue_identifier: "SIM-456",
        method: "approval",
        preview: "Review matching timestamp command",
        created_at: "2026-05-14T12:02:00Z",
        timeout_ms: 30000,
      },
      {
        request_id: "esc-2",
        issue_id: "issue-777",
        issue_identifier: "SIM-777",
        method: "approval",
        preview: "Review command",
        created_at: "2026-05-14T12:02:00Z",
        timeout_ms: 30000,
      },
      {
        request_id: "esc-1",
        issue_id: "issue-123",
        issue_identifier: "SIM-123",
        method: "approval",
        preview: "Approve command",
        created_at: "2026-05-14T12:01:00Z",
        timeout_ms: 600000,
      },
    ];

    const rows = buildEscalationRows(state);

    expect(rows.map((row) => row.requestId)).toEqual(["esc-1", "esc-2", "esc-3"]);
    expect(rows[0]).toMatchObject({
      method: "approval",
      preview: "Approve command",
      timeout: "10m 0s",
    });
  });

  it("formats escalation lifecycle events newest first", () => {
    const events: SymphonyEventEnvelope[] = [
      { version: "v1", sequence: 1, timestamp: "2026-05-14T12:00:00Z", kind: "escalation_created", severity: "info", issue: "SIM-123", event: "escalation_created", payload: { summary: "Approve command" } },
      { version: "v1", sequence: 2, timestamp: "2026-05-14T12:01:00Z", kind: "escalation_responded", severity: "info", issue: "SIM-123", event: "escalation_responded", payload: { summary: "Approved" } },
      { version: "v1", sequence: 3, timestamp: "2026-05-14T12:02:00Z", kind: "escalation_timed_out", severity: "warn", issue: "SIM-777", event: "escalation_timed_out", payload: { summary: "Timed out" } },
    ];

    expect(formatEventRows(events)).toEqual([
      "2026-05-14T12:02:00Z warn escalation_timed_out SIM-777 escalation_timed_out Timed out",
      "2026-05-14T12:01:00Z info escalation_responded SIM-123 escalation_responded Approved",
      "2026-05-14T12:00:00Z info escalation_created SIM-123 escalation_created Approve command",
    ]);
  });

  it("formats only recent worker and runtime events", () => {
    const events: SymphonyEventEnvelope[] = [
      { version: "v1", sequence: 1, timestamp: "2026-05-14T12:00:00Z", kind: "heartbeat", severity: "info", event: "heartbeat", payload: {} },
      { version: "v1", sequence: 2, timestamp: "2026-05-14T12:01:00Z", kind: "runtime", severity: "info", event: "poll_completed", payload: { summary: "checked tracker" } },
      { version: "v1", sequence: 3, timestamp: "2026-05-14T12:02:00Z", kind: "worker", severity: "error", issue: "SIM-123", event: "worker_failed", payload: { error_preview: "usage limit" } },
    ];

    expect(formatEventRows(events)).toEqual([
      "2026-05-14T12:02:00Z error worker SIM-123 worker_failed usage limit",
      "2026-05-14T12:01:00Z info runtime - poll_completed checked tracker",
    ]);
  });

  it("includes worker failure errors and steer instructions in event summaries", () => {
    const events: SymphonyEventEnvelope[] = [
      { version: "v1", sequence: 1, timestamp: "2026-05-14T12:00:00Z", kind: "worker", severity: "error", issue: "SIM-123", event: "worker_failed", payload: { error: "agent exited with code 1" } },
      { version: "v1", sequence: 2, timestamp: "2026-05-14T12:01:00Z", kind: "runtime", severity: "info", issue: "SIM-123", event: "steer_received", payload: { instruction_preview: "Focus on failing tests" } },
    ];

    expect(formatEventRows(events)).toEqual([
      "2026-05-14T12:01:00Z info runtime SIM-123 steer_received Focus on failing tests",
      "2026-05-14T12:00:00Z error worker SIM-123 worker_failed agent exited with code 1",
    ]);
  });

  it("normalizes multiline event summaries to one row", () => {
    const events: SymphonyEventEnvelope[] = [
      { version: "v1", sequence: 1, timestamp: "2026-05-14T12:00:00Z", kind: "worker", severity: "error", issue: "SIM-123", event: "worker_failed", payload: { error: "first line\n\tsecond line" } },
    ];

    expect(formatEventRows(events)).toEqual(["2026-05-14T12:00:00Z error worker SIM-123 worker_failed first line second line"]);
  });
});
