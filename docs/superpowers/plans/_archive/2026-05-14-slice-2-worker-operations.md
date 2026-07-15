---
type: Plan
title: Slice 2 Worker Operations
description: Archived implementation plan: Slice 2 Worker Operations.
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---

# Pi Symphony Extension Slice 2 Worker Operations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Slice 2 of `@kata-sh/pi-symphony-extension`: running worker table, selected-worker details, manual refresh, dashboard steering, and recent worker/runtime events.

**Architecture:** Extend the Slice 1 extension without changing Symphony Rust APIs. The HTTP client gains typed control methods for refresh and steer, the runtime keeps the latest raw state plus recent event envelopes in memory, and the dashboard renders worker operations from a focused dashboard model. Event streaming is isolated in one small module so dashboard rendering and WebSocket lifecycle stay testable.

**Tech Stack:** TypeScript, Pi extension APIs from `@earendil-works/pi-coding-agent`, TUI helpers from `@earendil-works/pi-tui`, Symphony HTTP API, `ws` for WebSocket event streaming, Vitest, pnpm workspace scripts.

---

## Source spec

Master design doc: `docs/superpowers/specs/2026-05-14-pi-symphony-extension-design.md`

Slice 2 requirements:

- Render the running workers table.
- Show selected-worker details: issue, tracker state, attempt, turn count, max turns, last activity, worker host, workspace path, and error preview.
- Support manual refresh.
- Support steering the selected worker from the dashboard.
- Show recent worker and runtime events.

Relevant existing files:

- `apps/symphony/pi-extension/src/dashboard.ts` currently renders Slice 1 health only and handles `r` as a local state refresh.
- `apps/symphony/pi-extension/src/http-client.ts` currently wraps `GET /api/v1/state` only.
- `apps/symphony/pi-extension/src/runtime.ts` currently stores only `lastKnownState`, not the raw Symphony state or events.
- `apps/symphony/pi-extension/src/commands.ts` and `src/tools.ts` do not yet expose refresh or steer.

## File structure

Create these files:

- `apps/symphony/pi-extension/src/dashboard-model.ts` — pure helpers that normalize Symphony state into worker rows, selected-worker details, and displayable event rows.
- `apps/symphony/pi-extension/src/dashboard-model.test.ts` — unit tests for worker row extraction, detail fields, event filtering, and text truncation.
- `apps/symphony/pi-extension/src/event-stream.ts` — WebSocket event stream client for `/api/v1/events`.
- `apps/symphony/pi-extension/src/event-stream.test.ts` — unit/integration tests for event stream URL generation, event parsing, and malformed message reporting.

Modify these files:

- `apps/symphony/pi-extension/package.json` — add `ws` runtime dependency and `@types/ws` dev dependency.
- `apps/symphony/pi-extension/src/http-client.ts` — add typed state details plus `POST /api/v1/refresh` and `POST /api/v1/steer` methods.
- `apps/symphony/pi-extension/src/http-client.test.ts` — add refresh, steer, and malformed control response tests.
- `apps/symphony/pi-extension/src/runtime.ts` — keep the latest raw state, recent event envelopes, refresh request behavior, and steer behavior.
- `apps/symphony/pi-extension/src/runtime.test.ts` — cover raw-state retention, event retention, refresh request, and steer request.
- `apps/symphony/pi-extension/src/state.ts` — default dashboard details to visible and preserve the setting through state restore.
- `apps/symphony/pi-extension/src/state.test.ts` — cover dashboard detail default/restore behavior if the current assertions need updating.
- `apps/symphony/pi-extension/src/dashboard.ts` — render worker table/details/events and implement keyboard controls for selection, details, refresh, and steer.
- `apps/symphony/pi-extension/src/dashboard.test.ts` — cover worker table rendering, selected details, selection movement, steering, refresh, and event rendering.
- `apps/symphony/pi-extension/src/command-args.ts` — add `/symphony:steer` argument parsing.
- `apps/symphony/pi-extension/src/command-args.test.ts` — cover steer parsing.
- `apps/symphony/pi-extension/src/commands.ts` — register `/symphony:refresh` and `/symphony:steer`.
- `apps/symphony/pi-extension/src/commands.test.ts` — cover refresh and steer command behavior.
- `apps/symphony/pi-extension/src/tools.ts` — register `symphony_refresh` and `symphony_steer` tools and include them in help output.
- `apps/symphony/pi-extension/src/tools.test.ts` — cover refresh/steer tool registration and behavior.
- `apps/symphony/pi-extension/README.md` — add Slice 2 command and dashboard key documentation.

Do not modify Symphony Rust source for Slice 2.

---

### Task 1: HTTP control surface

**Files:**
- Modify: `apps/symphony/pi-extension/package.json`
- Modify: `apps/symphony/pi-extension/src/http-client.ts`
- Modify: `apps/symphony/pi-extension/src/http-client.test.ts`

- [ ] **Step 1: Add the event stream dependency**

Run:

```bash
pnpm --dir apps/symphony/pi-extension add ws
pnpm --dir apps/symphony/pi-extension add -D @types/ws
```

Expected: `apps/symphony/pi-extension/package.json` contains `ws` under `dependencies` and `@types/ws` under `devDependencies`; the workspace lockfile is updated.

- [ ] **Step 2: Write failing tests for refresh and steer**

Add these tests inside `describe("SymphonyHttpClient", () => { ... })` in `apps/symphony/pi-extension/src/http-client.test.ts`:

```ts
  it("requests a Symphony poll refresh", async () => {
    const baseUrl = await serve((req) => {
      expect(req.method).toBe("POST");
      expect(req.url).toBe("/api/v1/refresh");
      return { status: 202, body: { queued: true, coalesced: false, pending_requests: 1 } };
    });

    const client = new SymphonyHttpClient(baseUrl);

    await expect(client.refresh()).resolves.toEqual({ queued: true, coalesced: false, pendingRequests: 1 });
  });

  it("sends a steer instruction for a running issue", async () => {
    const baseUrl = await serve((req, body) => {
      expect(req.method).toBe("POST");
      expect(req.url).toBe("/api/v1/steer");
      expect(JSON.parse(body)).toEqual({ issue_identifier: "SIM-123", instruction: "Use the existing auth module" });
      return {
        status: 200,
        body: {
          ok: true,
          issue_id: "issue-123",
          issue_identifier: "SIM-123",
          delivered: true,
          instruction_preview: "Use the existing auth module",
        },
      };
    });

    const client = new SymphonyHttpClient(baseUrl);

    await expect(client.steer("SIM-123", "Use the existing auth module")).resolves.toEqual({
      ok: true,
      issueId: "issue-123",
      issueIdentifier: "SIM-123",
      delivered: true,
      instructionPreview: "Use the existing auth module",
    });
  });

  it("rejects malformed refresh responses", async () => {
    const baseUrl = await serve(() => ({ status: 202, body: { queued: true } }));
    const client = new SymphonyHttpClient(baseUrl);

    await expect(client.refresh()).rejects.toMatchObject({
      kind: "non_symphony_response",
      message: "Response did not look like Symphony refresh response",
    } satisfies Partial<SymphonyExtensionError>);
  });

  it("normalizes steer API errors", async () => {
    const baseUrl = await serve(() => ({
      status: 404,
      body: { error: { code: "issue_not_running", message: "issue is not running", status: 404 } },
    }));
    const client = new SymphonyHttpClient(baseUrl);

    await expect(client.steer("SIM-404", "check logs")).rejects.toMatchObject({
      kind: "api_error",
      message: "issue is not running",
      details: expect.objectContaining({ code: "issue_not_running", status: 404 }),
    } satisfies Partial<SymphonyExtensionError>);
  });
```

- [ ] **Step 3: Run the new HTTP tests and verify they fail**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/http-client.test.ts -- --runInBand
```

Expected: FAIL because `client.refresh` and `client.steer` are not defined.

- [ ] **Step 4: Add response types and methods**

In `apps/symphony/pi-extension/src/http-client.ts`, replace the current `SymphonyStateResponse` interface block with this block:

```ts
export interface RunAttemptResponse {
  issue_id: string;
  issue_identifier: string;
  issue_title?: string | null;
  attempt?: number | null;
  workspace_path: string;
  started_at: string;
  status: string;
  error?: string | null;
  worker_host?: string | null;
  model?: string | null;
  tracker_state?: string | null;
  issue_url?: string | null;
}

export interface RunningSessionSnapshotResponse {
  turn_count?: number;
  last_activity_at?: string | null;
  total_tokens?: number;
  last_event?: string | null;
  last_event_message?: string | null;
  session_id?: string | null;
  current_tool_name?: string | null;
  current_tool_args_preview?: string | null;
  last_error?: string | null;
}

export interface WorkerSessionInfoResponse {
  turn_count?: number;
  max_turns?: number;
  last_activity_ms?: number | null;
  session_tokens?: {
    input_tokens?: number;
    output_tokens?: number;
    total_tokens?: number;
  };
  current_tool_name?: string | null;
  current_tool_args_preview?: string | null;
  last_error?: string | null;
}

export interface SymphonyStateResponse {
  tracker_project_url?: string | null;
  running?: Record<string, RunAttemptResponse>;
  running_sessions?: Record<string, RunningSessionSnapshotResponse>;
  running_session_info?: Record<string, WorkerSessionInfoResponse>;
  retry_queue?: unknown[];
  blocked?: unknown[];
  completed?: unknown[];
  polling?: {
    checking?: boolean;
    next_poll_in_ms?: number;
    poll_interval_ms?: number;
    poll_count?: number;
    last_poll_at?: string | null;
  };
}

export interface RefreshResponse {
  queued: boolean;
  coalesced: boolean;
  pendingRequests: number;
}

export interface SteerResponse {
  ok: boolean;
  issueId: string;
  issueIdentifier: string;
  delivered: boolean;
  instructionPreview: string;
}

export interface SymphonyEventEnvelope {
  version: string;
  sequence: number;
  timestamp: string;
  kind: string;
  severity: string;
  issue?: string;
  event: string;
  payload: unknown;
}
```

Then add these methods to `SymphonyHttpClient` after `verify`:

```ts
  async refresh(signal?: AbortSignal): Promise<RefreshResponse> {
    const path = "/api/v1/refresh";
    const json = await this.requestJson(path, { method: "POST", signal });
    return validateRefreshResponse(json, { baseUrl: this.baseUrl, path });
  }

  async steer(issueIdentifier: string, instruction: string, signal?: AbortSignal): Promise<SteerResponse> {
    const path = "/api/v1/steer";
    const json = await this.requestJson(path, {
      method: "POST",
      signal,
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ issue_identifier: issueIdentifier, instruction }),
    });
    return validateSteerResponse(json, { baseUrl: this.baseUrl, path, issueIdentifier });
  }
```

Add these validation functions after `validateSymphonyStateResponse`:

```ts
function validateRefreshResponse(value: unknown, details: Record<string, unknown>): RefreshResponse {
  if (!isRecord(value)) {
    throwNonSymphonyRefresh(details, "refresh response was not an object");
  }
  if (typeof value.queued !== "boolean") {
    throwNonSymphonyRefresh(details, "refresh response field had an invalid shape", { field: "queued", expected: "boolean" });
  }
  if (typeof value.coalesced !== "boolean") {
    throwNonSymphonyRefresh(details, "refresh response field had an invalid shape", { field: "coalesced", expected: "boolean" });
  }
  if (!isFiniteNumber(value.pending_requests)) {
    throwNonSymphonyRefresh(details, "refresh response field had an invalid shape", { field: "pending_requests", expected: "number" });
  }

  return {
    queued: value.queued,
    coalesced: value.coalesced,
    pendingRequests: value.pending_requests,
  };
}

function validateSteerResponse(value: unknown, details: Record<string, unknown>): SteerResponse {
  if (!isRecord(value)) {
    throwNonSymphonySteer(details, "steer response was not an object");
  }
  if (typeof value.ok !== "boolean") {
    throwNonSymphonySteer(details, "steer response field had an invalid shape", { field: "ok", expected: "boolean" });
  }
  if (typeof value.issue_id !== "string") {
    throwNonSymphonySteer(details, "steer response field had an invalid shape", { field: "issue_id", expected: "string" });
  }
  if (typeof value.issue_identifier !== "string") {
    throwNonSymphonySteer(details, "steer response field had an invalid shape", { field: "issue_identifier", expected: "string" });
  }
  if (typeof value.delivered !== "boolean") {
    throwNonSymphonySteer(details, "steer response field had an invalid shape", { field: "delivered", expected: "boolean" });
  }
  if (typeof value.instruction_preview !== "string") {
    throwNonSymphonySteer(details, "steer response field had an invalid shape", { field: "instruction_preview", expected: "string" });
  }

  return {
    ok: value.ok,
    issueId: value.issue_id,
    issueIdentifier: value.issue_identifier,
    delivered: value.delivered,
    instructionPreview: value.instruction_preview,
  };
}

function throwNonSymphonyRefresh(details: Record<string, unknown>, reason: string, extraDetails: Record<string, unknown> = {}): never {
  throw new SymphonyExtensionError("non_symphony_response", "Response did not look like Symphony refresh response", {
    ...details,
    reason,
    ...extraDetails,
  });
}

function throwNonSymphonySteer(details: Record<string, unknown>, reason: string, extraDetails: Record<string, unknown> = {}): never {
  throw new SymphonyExtensionError("non_symphony_response", "Response did not look like Symphony steer response", {
    ...details,
    reason,
    ...extraDetails,
  });
}
```

- [ ] **Step 5: Run HTTP tests and verify they pass**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/http-client.test.ts -- --runInBand
```

Expected: PASS for all `SymphonyHttpClient` tests.

- [ ] **Step 6: Commit HTTP control surface**

```bash
git add apps/symphony/pi-extension/package.json pnpm-lock.yaml apps/symphony/pi-extension/src/http-client.ts apps/symphony/pi-extension/src/http-client.test.ts
git commit -m "feat: add Symphony worker control HTTP methods"
```

---

### Task 2: Runtime worker state and dashboard model

**Files:**
- Create: `apps/symphony/pi-extension/src/dashboard-model.ts`
- Create: `apps/symphony/pi-extension/src/dashboard-model.test.ts`
- Modify: `apps/symphony/pi-extension/src/runtime.ts`
- Modify: `apps/symphony/pi-extension/src/runtime.test.ts`
- Modify: `apps/symphony/pi-extension/src/state.ts`
- Modify: `apps/symphony/pi-extension/src/state.test.ts`

- [ ] **Step 1: Write failing dashboard model tests**

Create `apps/symphony/pi-extension/src/dashboard-model.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { buildWorkerRows, formatEventRows } from "./dashboard-model.ts";
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
        error: "agent exited after a very long error message that needs to be shortened for the dashboard",
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
  };
}

describe("dashboard model", () => {
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
    expect(rows[1].attempt).toBe("1");
    expect(rows[1].trackerState).toBe("-");
    expect(rows[1].errorPreview).toBe("agent exited after a very long error message that needs to be shortened for the dashboard");
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
});
```

- [ ] **Step 2: Run dashboard model tests and verify they fail**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/dashboard-model.test.ts
```

Expected: FAIL because `dashboard-model.ts` does not exist.

- [ ] **Step 3: Implement the dashboard model**

Create `apps/symphony/pi-extension/src/dashboard-model.ts`:

```ts
import type { RunAttemptResponse, SymphonyEventEnvelope, SymphonyStateResponse } from "./http-client.ts";

export interface WorkerRow {
  issueId: string;
  issueIdentifier: string;
  title: string;
  trackerState: string;
  attempt: string;
  turnCount: string;
  maxTurns: string;
  lastActivity: string;
  workerHost: string;
  workspacePath: string;
  status: string;
  errorPreview: string;
}

export function buildWorkerRows(state: SymphonyStateResponse | undefined): WorkerRow[] {
  return Object.entries(state?.running ?? {})
    .map(([issueId, attempt]) => buildWorkerRow(issueId, attempt, state))
    .sort((left, right) => left.issueIdentifier.localeCompare(right.issueIdentifier));
}

export function formatEventRows(events: SymphonyEventEnvelope[], limit = 8): string[] {
  return events
    .filter((event) => event.kind === "worker" || event.kind === "runtime")
    .slice(-limit)
    .reverse()
    .map((event) => [event.timestamp, event.severity, event.kind, event.issue ?? "-", event.event, eventSummary(event)].filter(Boolean).join(" "));
}

function buildWorkerRow(issueId: string, attempt: RunAttemptResponse, state: SymphonyStateResponse | undefined): WorkerRow {
  const session = state?.running_sessions?.[issueId];
  const info = state?.running_session_info?.[issueId];
  const turnCount = info?.turn_count ?? session?.turn_count;
  const lastError = attempt.error ?? info?.last_error ?? session?.last_error;

  return {
    issueId,
    issueIdentifier: attempt.issue_identifier,
    title: attempt.issue_title ?? "-",
    trackerState: attempt.tracker_state ?? "-",
    attempt: String(attempt.attempt ?? 1),
    turnCount: turnCount === undefined ? "-" : String(turnCount),
    maxTurns: info?.max_turns === undefined ? "-" : String(info.max_turns),
    lastActivity: formatLastActivity(info?.last_activity_ms, session?.last_activity_at),
    workerHost: attempt.worker_host ?? "local",
    workspacePath: attempt.workspace_path,
    status: attempt.status,
    errorPreview: lastError ? truncateText(lastError, 120) : "-",
  };
}

function formatLastActivity(lastActivityMs: number | null | undefined, lastActivityAt: string | null | undefined): string {
  if (typeof lastActivityMs === "number" && Number.isFinite(lastActivityMs)) {
    return new Date(lastActivityMs).toISOString();
  }
  return lastActivityAt ?? "-";
}

function eventSummary(event: SymphonyEventEnvelope): string {
  if (!isRecord(event.payload)) return "";
  const preferred = event.payload.error_preview ?? event.payload.summary ?? event.payload.message ?? event.payload.reason;
  return typeof preferred === "string" ? truncateText(preferred, 120) : "";
}

function truncateText(value: string, maxLength: number): string {
  if (value.length <= maxLength) return value;
  return `${value.slice(0, maxLength - 1)}…`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
```

- [ ] **Step 4: Write failing runtime tests**

Add these tests to `apps/symphony/pi-extension/src/runtime.test.ts`:

```ts
  it("keeps the latest raw Symphony state after refresh", async () => {
    const runtime = new SymphonyRuntime();
    const response = {
      tracker_project_url: "https://linear.app/kata-sh/project/symphony",
      running: {},
      retry_queue: [],
      blocked: [],
      completed: [],
      polling: { checking: false, next_poll_in_ms: 1000, poll_interval_ms: 30000 },
    };
    runtime.client = {
      getState: vi.fn(async () => response),
      toHealthSummary: vi.fn(() => lastKnownState("http://127.0.0.1:8080")),
    } as unknown as SymphonyRuntime["client"];

    await expect(runtime.refreshState()).resolves.toBe(response);

    expect(runtime.lastState).toBe(response);
    expect(runtime.state.lastKnownState?.runningCount).toBe(1);
  });

  it("requests a refresh before fetching the latest state", async () => {
    const runtime = new SymphonyRuntime();
    const calls: string[] = [];
    const response = {
      running: {},
      retry_queue: [],
      blocked: [],
      completed: [],
      polling: { checking: false, next_poll_in_ms: 1000, poll_interval_ms: 30000 },
    };
    runtime.client = {
      refresh: vi.fn(async () => {
        calls.push("refresh");
        return { queued: true, coalesced: false, pendingRequests: 1 };
      }),
      getState: vi.fn(async () => {
        calls.push("getState");
        return response;
      }),
      toHealthSummary: vi.fn(() => lastKnownState("http://127.0.0.1:8080")),
    } as unknown as SymphonyRuntime["client"];

    await runtime.requestRefresh();

    expect(calls).toEqual(["refresh", "getState"]);
    expect(runtime.lastState).toBe(response);
  });

  it("steers a worker and refreshes state", async () => {
    const runtime = new SymphonyRuntime();
    const response = {
      running: {},
      retry_queue: [],
      blocked: [],
      completed: [],
      polling: { checking: false, next_poll_in_ms: 1000, poll_interval_ms: 30000 },
    };
    runtime.client = {
      steer: vi.fn(async () => ({ ok: true, issueId: "issue-123", issueIdentifier: "SIM-123", delivered: true, instructionPreview: "Use auth" })),
      getState: vi.fn(async () => response),
      toHealthSummary: vi.fn(() => lastKnownState("http://127.0.0.1:8080")),
    } as unknown as SymphonyRuntime["client"];

    await expect(runtime.steerWorker("SIM-123", "Use auth")).resolves.toMatchObject({ delivered: true });

    expect(runtime.client?.steer).toHaveBeenCalledWith("SIM-123", "Use auth", undefined);
    expect(runtime.client?.getState).toHaveBeenCalledOnce();
  });

  it("retains the most recent worker and runtime events", () => {
    const runtime = new SymphonyRuntime();

    runtime.recordEvent({ version: "v1", sequence: 1, timestamp: "2026-05-14T12:00:00Z", kind: "heartbeat", severity: "info", event: "heartbeat", payload: {} });
    for (let sequence = 2; sequence <= 27; sequence += 1) {
      runtime.recordEvent({ version: "v1", sequence, timestamp: `2026-05-14T12:00:${String(sequence).padStart(2, "0")}Z`, kind: sequence % 2 === 0 ? "worker" : "runtime", severity: "info", event: "event", payload: {} });
    }

    expect(runtime.recentEvents).toHaveLength(20);
    expect(runtime.recentEvents[0].sequence).toBe(8);
    expect(runtime.recentEvents.at(-1)?.sequence).toBe(27);
  });
```

Also update the imports in `runtime.test.ts` from:

```ts
import { describe, expect, it } from "vitest";
```

to:

```ts
import { describe, expect, it, vi } from "vitest";
```

- [ ] **Step 5: Run runtime tests and verify they fail**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/runtime.test.ts
```

Expected: FAIL because `lastState`, `requestRefresh`, `steerWorker`, `recordEvent`, and `recentEvents` are not implemented.

- [ ] **Step 6: Implement runtime state, refresh, steer, and event retention**

In `apps/symphony/pi-extension/src/runtime.ts`, update the HTTP client import to include the event and response types:

```ts
import { SymphonyHttpClient, type SteerResponse, type SymphonyEventEnvelope, type SymphonyStateResponse } from "./http-client.ts";
```

Add these fields near the top of `SymphonyRuntime`:

```ts
  lastState?: SymphonyStateResponse;
  recentEvents: SymphonyEventEnvelope[] = [];
```

In `attach`, after `const state = await client.verify(signal);`, add:

```ts
    this.lastState = state;
```

Replace `clearAttachment` with:

```ts
  clearAttachment(): void {
    this.client = undefined;
    this.state.attachedBaseUrl = undefined;
    this.state.lastKnownState = undefined;
    this.lastState = undefined;
    this.recentEvents = [];
  }
```

Replace `refreshState` with:

```ts
  async refreshState(signal?: AbortSignal): Promise<SymphonyStateResponse> {
    if (!this.client) throw new SymphonyExtensionError("no_attachment", "No Symphony server is attached");
    const state = await this.client.getState(signal);
    this.lastState = state;
    this.state.lastKnownState = this.client.toHealthSummary(state);
    return state;
  }
```

Add these methods after `refreshState`:

```ts
  async requestRefresh(signal?: AbortSignal): Promise<SymphonyStateResponse> {
    if (!this.client) throw new SymphonyExtensionError("no_attachment", "No Symphony server is attached");
    await this.client.refresh(signal);
    return this.refreshState(signal);
  }

  async steerWorker(issueIdentifier: string, instruction: string, signal?: AbortSignal): Promise<SteerResponse> {
    if (!this.client) throw new SymphonyExtensionError("no_attachment", "No Symphony server is attached");
    const result = await this.client.steer(issueIdentifier, instruction, signal);
    await this.refreshState(signal);
    return result;
  }

  recordEvent(event: SymphonyEventEnvelope): void {
    if (event.kind !== "worker" && event.kind !== "runtime") return;
    this.recentEvents = [...this.recentEvents, event].slice(-20);
  }
```

- [ ] **Step 7: Make dashboard details visible by default**

In `apps/symphony/pi-extension/src/state.ts`, replace:

```ts
    dashboard: { showDetails: false },
```

with:

```ts
    dashboard: { showDetails: true },
```

If `apps/symphony/pi-extension/src/state.test.ts` asserts the previous default, update that assertion to expect `true` so selected-worker details are visible on first dashboard open.

- [ ] **Step 8: Run model, runtime, and state tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/dashboard-model.test.ts src/runtime.test.ts src/state.test.ts
```

Expected: PASS for dashboard model, runtime, and state tests.

- [ ] **Step 9: Commit runtime and model changes**

```bash
git add apps/symphony/pi-extension/src/dashboard-model.ts apps/symphony/pi-extension/src/dashboard-model.test.ts apps/symphony/pi-extension/src/runtime.ts apps/symphony/pi-extension/src/runtime.test.ts apps/symphony/pi-extension/src/state.ts apps/symphony/pi-extension/src/state.test.ts
git commit -m "feat: model Symphony worker dashboard state"
```

---

### Task 3: Worker operations dashboard UI

**Files:**
- Modify: `apps/symphony/pi-extension/src/dashboard.ts`
- Modify: `apps/symphony/pi-extension/src/dashboard.test.ts`

- [ ] **Step 1: Write failing dashboard rendering tests**

Add these helpers and tests to `apps/symphony/pi-extension/src/dashboard.test.ts`.

Add imports:

```ts
import type { SymphonyEventEnvelope, SymphonyStateResponse } from "./http-client.ts";
```

Add this fixture near the top of the file:

```ts
function workerStateFixture(): SymphonyStateResponse {
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
        error: "usage limit",
        tracker_state: "Agent Review",
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
  };
}

function runtimeEventsFixture(): SymphonyEventEnvelope[] {
  return [
    { version: "v1", sequence: 1, timestamp: "2026-05-14T12:01:00Z", kind: "runtime", severity: "info", event: "poll_completed", payload: { summary: "checked tracker" } },
    { version: "v1", sequence: 2, timestamp: "2026-05-14T12:02:00Z", kind: "worker", severity: "error", issue: "SIM-777", event: "worker_failed", payload: { error_preview: "usage limit" } },
  ];
}
```

Add these tests inside `describe("SymphonyDashboardComponent", () => { ... })`:

```ts
  it("renders running workers, selected-worker details, and recent runtime events", () => {
    const state = createDefaultState();
    state.dashboard.showDetails = true;
    const dashboard = new SymphonyDashboardComponent({
      state,
      getState: () => workerStateFixture(),
      getEvents: () => runtimeEventsFixture(),
      refresh: async () => undefined,
      steer: async () => undefined,
      prompt: async () => undefined,
      close: () => undefined,
      requestRender: () => undefined,
      notify: () => undefined,
    });

    const output = dashboard.render(160).join("\n");

    expect(output).toContain("Running workers");
    expect(output).toContain("> SIM-123");
    expect(output).toContain("SIM-777");
    expect(output).toContain("Selected worker");
    expect(output).toContain("issue: SIM-123 Worker one");
    expect(output).toContain("tracker state: In Progress");
    expect(output).toContain("attempt: 2");
    expect(output).toContain("turns: 3 / 20");
    expect(output).toContain("last activity: 2026-05-14T12:04:00.000Z");
    expect(output).toContain("worker host: worker-a");
    expect(output).toContain("workspace: /tmp/symphony/issue-123");
    expect(output).toContain("Recent worker/runtime events");
    expect(output).toContain("worker_failed usage limit");
  });

  it("moves selection with arrow keys", () => {
    const state = createDefaultState();
    const dashboard = new SymphonyDashboardComponent({
      state,
      getState: () => workerStateFixture(),
      getEvents: () => [],
      refresh: async () => undefined,
      steer: async () => undefined,
      prompt: async () => undefined,
      close: () => undefined,
      requestRender: () => undefined,
      notify: () => undefined,
    });

    dashboard.handleInput("\u001b[B");

    const output = dashboard.render(160).join("\n");
    expect(output).toContain("> SIM-777");
    expect(output).toContain("issue: SIM-777 Worker two");
  });

  it("toggles selected-worker details", () => {
    const state = createDefaultState();
    state.dashboard.showDetails = true;
    const dashboard = new SymphonyDashboardComponent({
      state,
      getState: () => workerStateFixture(),
      getEvents: () => [],
      refresh: async () => undefined,
      steer: async () => undefined,
      prompt: async () => undefined,
      close: () => undefined,
      requestRender: () => undefined,
      notify: () => undefined,
    });

    dashboard.handleInput("d");

    expect(dashboard.render(160).join("\n")).not.toContain("Selected worker");
  });
```

- [ ] **Step 2: Write failing dashboard steering test**

Add this test inside `describe("SymphonyDashboardComponent", () => { ... })`:

```ts
  it("prompts for a steer instruction and sends it to the selected worker", async () => {
    let resolveSteered: (() => void) | undefined;
    const steered = new Promise<void>((resolve) => {
      resolveSteered = resolve;
    });
    const steer = vi.fn(async () => {
      resolveSteered?.();
    });
    const notify = vi.fn();
    const dashboard = new SymphonyDashboardComponent({
      state: createDefaultState(),
      getState: () => workerStateFixture(),
      getEvents: () => [],
      refresh: async () => undefined,
      steer,
      prompt: async () => "Use the existing auth module",
      close: () => undefined,
      requestRender: () => undefined,
      notify,
    });

    dashboard.handleInput("s");
    await steered;

    expect(steer).toHaveBeenCalledWith("SIM-123", "Use the existing auth module");
    expect(notify).toHaveBeenCalledWith("Steer delivered to SIM-123", "info");
  });
```

- [ ] **Step 3: Run dashboard tests and verify they fail**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/dashboard.test.ts
```

Expected: FAIL because `DashboardOptions` does not include `getState`, `getEvents`, `steer`, or `prompt`, and dashboard rendering still only shows Slice 1 health.

- [ ] **Step 4: Implement dashboard options, selection, details, refresh, and steering**

In `apps/symphony/pi-extension/src/dashboard.ts`, update imports:

```ts
import { buildWorkerRows, formatEventRows, type WorkerRow } from "./dashboard-model.ts";
import type { SymphonyEventEnvelope, SymphonyStateResponse } from "./http-client.ts";
```

Replace `DashboardOptions` with:

```ts
export interface DashboardOptions {
  state: ExtensionState;
  getState: () => SymphonyStateResponse | undefined;
  getEvents: () => SymphonyEventEnvelope[];
  refresh: () => Promise<void>;
  steer: (issueIdentifier: string, instruction: string) => Promise<void>;
  prompt: (title: string, label: string) => Promise<string | undefined>;
  close: () => void;
  requestRender: () => void;
  notify: (message: string, level: "info" | "warning" | "error") => void;
}
```

Add this field to `SymphonyDashboardComponent`:

```ts
  private selectedWorkerIndex = 0;
```

Replace `handleInput` with:

```ts
  handleInput(data: string): void {
    if (data === "q" || data === "Q" || matchesKey(data, "escape")) {
      this.options.close();
      return;
    }

    if (data === "r" || data === "R") {
      void this.refresh();
      return;
    }

    if (data === "d" || data === "D") {
      this.options.state.dashboard.showDetails = !this.options.state.dashboard.showDetails;
      this.options.requestRender();
      return;
    }

    if (data === "s" || data === "S") {
      void this.steerSelectedWorker();
      return;
    }

    if (data === "\u001b[A" || matchesKey(data, "up")) {
      this.moveSelection(-1);
      return;
    }

    if (data === "\u001b[B" || matchesKey(data, "down")) {
      this.moveSelection(1);
    }
  }
```

Replace `render` with:

```ts
  render(width: number): string[] {
    const state = this.options.state;
    const health = state.lastKnownState;
    const workers = buildWorkerRows(this.options.getState());
    this.clampSelection(workers.length);
    const selectedWorker = workers[this.selectedWorkerIndex];
    const lines = [
      "Symphony Dashboard",
      "",
      `connection: ${state.attachedBaseUrl ? "attached" : "detached"}`,
      `base url: ${state.attachedBaseUrl ?? "none"}`,
      `project: ${health?.trackerProjectUrl ?? "none"}`,
      `polling: ${health?.pollingChecking ? "checking" : "idle"} | next poll: ${health?.nextPollInMs ?? 0}ms`,
      `workers: running: ${health?.runningCount ?? workers.length} | retry: ${health?.retryCount ?? 0} | blocked: ${health?.blockedCount ?? 0} | completed: ${health?.completedCount ?? 0}`,
      `owned process: ${state.ownedProcess ? `pid ${state.ownedProcess.pid}` : "none"}`,
      `updated: ${health?.updatedAt ?? "never"}`,
      "",
      this.refreshing ? "refreshing..." : "keys: ↑/↓ select | r refresh | s steer | d details | q/esc close",
      "",
      ...renderWorkerTable(workers, this.selectedWorkerIndex),
      ...renderSelectedWorkerDetails(selectedWorker, state.dashboard.showDetails),
      ...renderRecentEvents(formatEventRows(this.options.getEvents())),
    ];

    return lines.map((line) => truncateToWidth(line, width));
  }
```

Add these methods inside `SymphonyDashboardComponent`:

```ts
  private moveSelection(delta: number): void {
    const workers = buildWorkerRows(this.options.getState());
    if (workers.length === 0) return;
    this.selectedWorkerIndex = Math.max(0, Math.min(workers.length - 1, this.selectedWorkerIndex + delta));
    this.options.requestRender();
  }

  private clampSelection(workerCount: number): void {
    if (workerCount === 0) {
      this.selectedWorkerIndex = 0;
      return;
    }
    this.selectedWorkerIndex = Math.max(0, Math.min(workerCount - 1, this.selectedWorkerIndex));
  }

  private async steerSelectedWorker(): Promise<void> {
    const workers = buildWorkerRows(this.options.getState());
    this.clampSelection(workers.length);
    const worker = workers[this.selectedWorkerIndex];
    if (!worker) {
      this.options.notify("No running worker is selected", "warning");
      return;
    }

    const instruction = (await this.options.prompt("Steer Symphony worker", `Instruction for ${worker.issueIdentifier}`))?.trim();
    if (!instruction) return;

    try {
      await this.options.steer(worker.issueIdentifier, instruction);
      this.options.notify(`Steer delivered to ${worker.issueIdentifier}`, "info");
    } catch (error) {
      this.options.notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      this.options.requestRender();
    }
  }
```

Add these helper functions after the class:

```ts
function renderWorkerTable(workers: WorkerRow[], selectedIndex: number): string[] {
  const lines = ["Running workers", "sel issue    state           attempt turns   host      last activity"];
  if (workers.length === 0) return [...lines, "-   no running workers"];

  for (const [index, worker] of workers.entries()) {
    const selected = index === selectedIndex ? ">" : " ";
    lines.push([
      selected,
      pad(worker.issueIdentifier, 8),
      pad(worker.trackerState, 15),
      pad(worker.attempt, 7),
      pad(`${worker.turnCount}/${worker.maxTurns}`, 7),
      pad(worker.workerHost, 9),
      worker.lastActivity,
    ].join(" "));
  }
  return lines;
}

function renderSelectedWorkerDetails(worker: WorkerRow | undefined, showDetails: boolean): string[] {
  if (!showDetails) return [];
  if (!worker) return ["", "Selected worker", "none"];
  return [
    "",
    "Selected worker",
    `issue: ${worker.issueIdentifier} ${worker.title}`,
    `tracker state: ${worker.trackerState}`,
    `attempt: ${worker.attempt}`,
    `turns: ${worker.turnCount} / ${worker.maxTurns}`,
    `last activity: ${worker.lastActivity}`,
    `worker host: ${worker.workerHost}`,
    `workspace: ${worker.workspacePath}`,
    `error: ${worker.errorPreview}`,
  ];
}

function renderRecentEvents(events: string[]): string[] {
  const lines = ["", "Recent worker/runtime events"];
  if (events.length === 0) return [...lines, "none"];
  return [...lines, ...events];
}

function pad(value: string, width: number): string {
  if (value.length >= width) return value.slice(0, width);
  return value.padEnd(width, " ");
}
```

In `openDashboard`, update the component construction so it passes the new options and uses POST refresh for manual refresh:

```ts
    const component = new SymphonyDashboardComponent({
      state: runtime.state,
      getState: () => runtime.lastState,
      getEvents: () => runtime.recentEvents,
      refresh: async () => {
        await runtime.requestRefresh();
      },
      steer: async (issueIdentifier, instruction) => {
        await runtime.steerWorker(issueIdentifier, instruction);
      },
      prompt: async (title, label) => ctx.ui.input(title, label),
      close: () => done(undefined),
      requestRender: () => tui.requestRender(),
      notify: (message, level) => ctx.ui.notify(message, level),
    });
```

Update existing dashboard tests that construct `SymphonyDashboardComponent` by adding these default options:

```ts
      getState: () => undefined,
      getEvents: () => [],
      steer: async () => undefined,
      prompt: async () => undefined,
```

- [ ] **Step 5: Run dashboard tests and verify they pass**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/dashboard.test.ts
```

Expected: PASS for dashboard rendering, selection, detail toggle, refresh, and steer tests.

- [ ] **Step 6: Commit dashboard UI changes**

```bash
git add apps/symphony/pi-extension/src/dashboard.ts apps/symphony/pi-extension/src/dashboard.test.ts
git commit -m "feat: render Symphony worker operations dashboard"
```

---

### Task 4: Recent worker and runtime events

**Files:**
- Create: `apps/symphony/pi-extension/src/event-stream.ts`
- Create: `apps/symphony/pi-extension/src/event-stream.test.ts`
- Modify: `apps/symphony/pi-extension/src/dashboard.ts`
- Modify: `apps/symphony/pi-extension/src/dashboard.test.ts`

- [ ] **Step 1: Write failing event stream tests**

Create `apps/symphony/pi-extension/src/event-stream.test.ts`:

```ts
import { createServer, type Server } from "node:http";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WebSocketServer } from "ws";
import { eventStreamUrl, startSymphonyEventStream } from "./event-stream.ts";

let server: Server | undefined;
let socketServer: WebSocketServer | undefined;

afterEach(async () => {
  socketServer?.close();
  socketServer = undefined;
  if (server) {
    await new Promise<void>((resolve, reject) => server!.close((error) => (error ? reject(error) : resolve())));
    server = undefined;
  }
});

async function serveWebSocket(): Promise<{ baseUrl: string; socketServer: WebSocketServer }> {
  server = createServer();
  socketServer = new WebSocketServer({ server, path: "/api/v1/events" });
  await new Promise<void>((resolve) => server!.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("expected TCP address");
  return { baseUrl: `http://127.0.0.1:${address.port}`, socketServer };
}

describe("event stream", () => {
  it("converts HTTP API base URLs into WebSocket event URLs", () => {
    expect(eventStreamUrl("http://127.0.0.1:8080")).toBe("ws://127.0.0.1:8080/api/v1/events");
    expect(eventStreamUrl("https://example.test/base")).toBe("wss://example.test/base/api/v1/events");
  });

  it("delivers parsed Symphony event envelopes", async () => {
    const { baseUrl, socketServer } = await serveWebSocket();
    const received = new Promise<unknown>((resolve) => {
      const handle = startSymphonyEventStream({
        baseUrl,
        onEvent: (event) => {
          handle.close();
          resolve(event);
        },
        onError: (error) => resolve(error),
      });
    });

    socketServer.on("connection", (socket) => {
      socket.send(JSON.stringify({
        version: "v1",
        sequence: 7,
        timestamp: "2026-05-14T12:00:00Z",
        kind: "worker",
        severity: "info",
        issue: "SIM-123",
        event: "worker_completed",
        payload: { summary: "done" },
      }));
    });

    await expect(received).resolves.toMatchObject({ sequence: 7, kind: "worker", event: "worker_completed" });
  });

  it("reports malformed event stream messages", async () => {
    const { baseUrl, socketServer } = await serveWebSocket();
    const onError = vi.fn();
    const reported = new Promise<void>((resolve) => {
      onError.mockImplementation(() => resolve());
    });
    const handle = startSymphonyEventStream({ baseUrl, onEvent: () => undefined, onError });

    socketServer.on("connection", (socket) => {
      socket.send("not-json");
    });

    await reported;
    handle.close();

    expect(onError).toHaveBeenCalledWith(expect.objectContaining({ message: expect.stringContaining("invalid Symphony event") }));
  });
});
```

- [ ] **Step 2: Run event stream tests and verify they fail**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/event-stream.test.ts -- --runInBand
```

Expected: FAIL because `event-stream.ts` does not exist.

- [ ] **Step 3: Implement event stream client**

Create `apps/symphony/pi-extension/src/event-stream.ts`:

```ts
import WebSocket from "ws";
import type { SymphonyEventEnvelope } from "./http-client.ts";

export interface EventStreamOptions {
  baseUrl: string;
  onEvent: (event: SymphonyEventEnvelope) => void;
  onError: (error: Error) => void;
}

export interface EventStreamHandle {
  close: () => void;
}

export function eventStreamUrl(baseUrl: string): string {
  const url = new URL(baseUrl);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.pathname = `${url.pathname.replace(/\/+$/, "")}/api/v1/events`;
  url.search = "";
  url.hash = "";
  return url.toString();
}

export function startSymphonyEventStream(options: EventStreamOptions): EventStreamHandle {
  const socket = new WebSocket(eventStreamUrl(options.baseUrl));

  socket.on("message", (data) => {
    try {
      options.onEvent(parseEventEnvelope(data.toString()));
    } catch (error) {
      options.onError(error instanceof Error ? error : new Error(String(error)));
    }
  });

  socket.on("error", (error) => {
    options.onError(error instanceof Error ? error : new Error(String(error)));
  });

  return {
    close: () => {
      socket.close();
    },
  };
}

function parseEventEnvelope(text: string): SymphonyEventEnvelope {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch (error) {
    throw new Error(`invalid Symphony event JSON: ${error instanceof Error ? error.message : String(error)}`);
  }

  if (!isRecord(value)) throw new Error("invalid Symphony event: envelope was not an object");
  if (value.version !== "v1") throw new Error("invalid Symphony event: version was not v1");
  if (typeof value.sequence !== "number") throw new Error("invalid Symphony event: sequence was not a number");
  if (typeof value.timestamp !== "string") throw new Error("invalid Symphony event: timestamp was not a string");
  if (typeof value.kind !== "string") throw new Error("invalid Symphony event: kind was not a string");
  if (typeof value.severity !== "string") throw new Error("invalid Symphony event: severity was not a string");
  if (value.issue !== undefined && typeof value.issue !== "string") throw new Error("invalid Symphony event: issue was not a string");
  if (typeof value.event !== "string") throw new Error("invalid Symphony event: event was not a string");

  return {
    version: value.version,
    sequence: value.sequence,
    timestamp: value.timestamp,
    kind: value.kind,
    severity: value.severity,
    issue: value.issue,
    event: value.event,
    payload: value.payload,
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
```

- [ ] **Step 4: Integrate event stream lifecycle into dashboard open/close**

In `apps/symphony/pi-extension/src/dashboard.ts`, add this import:

```ts
import { startSymphonyEventStream, type EventStreamHandle } from "./event-stream.ts";
```

Inside the `ctx.ui.custom` factory in `openDashboard`, before `const component = new SymphonyDashboardComponent`, add:

```ts
    let eventStream: EventStreamHandle | undefined;
    let eventStreamErrorNotified = false;
    if (runtime.state.attachedBaseUrl) {
      eventStream = startSymphonyEventStream({
        baseUrl: runtime.state.attachedBaseUrl,
        onEvent: (event) => {
          runtime.recordEvent(event);
          tui.requestRender();
        },
        onError: (error) => {
          if (eventStreamErrorNotified) return;
          eventStreamErrorNotified = true;
          ctx.ui.notify(`Symphony event stream unavailable: ${error.message}`, "warning");
        },
      });
    }
```

Then replace the `close` option from:

```ts
      close: () => done(undefined),
```

to:

```ts
      close: () => {
        eventStream?.close();
        done(undefined);
      },
```

- [ ] **Step 5: Add dashboard open test coverage for event stream integration**

In `apps/symphony/pi-extension/src/dashboard.test.ts`, add:

```ts
vi.mock("./event-stream.ts", () => ({
  startSymphonyEventStream: vi.fn(() => ({ close: vi.fn() })),
}));
```

Add this import near the top:

```ts
import { startSymphonyEventStream } from "./event-stream.ts";
```

Add this test inside `describe("openDashboard", () => { ... })`:

```ts
  it("opens the event stream and records incoming dashboard events", async () => {
    const state = createDefaultState();
    state.attachedBaseUrl = "http://127.0.0.1:8080";
    state.lastKnownState = {
      baseUrl: state.attachedBaseUrl,
      runningCount: 1,
      retryCount: 0,
      blockedCount: 0,
      completedCount: 0,
      pollingChecking: false,
      nextPollInMs: 1000,
      updatedAt: "2026-05-14T12:00:00Z",
    };

    let capturedOnEvent: ((event: SymphonyEventEnvelope) => void) | undefined;
    vi.mocked(startSymphonyEventStream).mockImplementation((options) => {
      capturedOnEvent = options.onEvent;
      return { close: vi.fn() };
    });

    type CustomFactory = Parameters<ExtensionContext["ui"]["custom"]>[0];
    const requestRender = vi.fn();
    const custom = vi.fn(async (factory: CustomFactory): Promise<void> => {
      const component = await factory(
        { requestRender } as unknown as Parameters<CustomFactory>[0],
        {} as Parameters<CustomFactory>[1],
        {} as Parameters<CustomFactory>[2],
        (() => undefined) as Parameters<CustomFactory>[3],
      );
      capturedOnEvent?.({ version: "v1", sequence: 1, timestamp: "2026-05-14T12:00:00Z", kind: "worker", severity: "info", issue: "SIM-123", event: "worker_started", payload: {} });
      expect(component.render(120).join("\n")).toContain("worker_started");
    });
    const ctx = { ui: { notify: vi.fn(), custom, input: vi.fn() } } as unknown as ExtensionContext;
    const runtime = {
      client: {},
      state,
      lastState: workerStateFixture(),
      recentEvents: [],
      recordEvent: vi.fn(function (this: { recentEvents: SymphonyEventEnvelope[] }, event: SymphonyEventEnvelope) {
        this.recentEvents.push(event);
      }),
      requestRefresh: vi.fn(async () => undefined),
      refreshState: vi.fn(async () => workerStateFixture()),
      steerWorker: vi.fn(async () => undefined),
      errorText: vi.fn((error: unknown) => (error instanceof Error ? error.message : String(error))),
    } as unknown as SymphonyRuntime;

    await openDashboard(ctx, runtime);

    expect(startSymphonyEventStream).toHaveBeenCalledWith(expect.objectContaining({ baseUrl: "http://127.0.0.1:8080" }));
    expect(requestRender).toHaveBeenCalled();
  });
```

- [ ] **Step 6: Run event and dashboard tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/event-stream.test.ts src/dashboard.test.ts -- --runInBand
```

Expected: PASS for event stream and dashboard tests.

- [ ] **Step 7: Commit recent event support**

```bash
git add apps/symphony/pi-extension/src/event-stream.ts apps/symphony/pi-extension/src/event-stream.test.ts apps/symphony/pi-extension/src/dashboard.ts apps/symphony/pi-extension/src/dashboard.test.ts
git commit -m "feat: stream Symphony worker events into dashboard"
```

---

### Task 5: Refresh and steer commands/tools

**Files:**
- Modify: `apps/symphony/pi-extension/src/command-args.ts`
- Modify: `apps/symphony/pi-extension/src/command-args.test.ts`
- Modify: `apps/symphony/pi-extension/src/commands.ts`
- Modify: `apps/symphony/pi-extension/src/commands.test.ts`
- Modify: `apps/symphony/pi-extension/src/tools.ts`
- Modify: `apps/symphony/pi-extension/src/tools.test.ts`
- Modify: `apps/symphony/pi-extension/README.md`

- [ ] **Step 1: Write failing command argument tests**

In `apps/symphony/pi-extension/src/command-args.test.ts`, update the import to include `parseSteerArgs`:

```ts
import { parseAttachArgs, parseDoctorArgs, parseInitArgs, parseStartArgs, parseSteerArgs } from "./command-args.ts";
```

Add this test:

```ts
  it("parses steer issue and instruction", () => {
    expect(parseSteerArgs("SIM-123 Use the existing auth module")).toEqual({
      issueIdentifier: "SIM-123",
      instruction: "Use the existing auth module",
    });
    expect(() => parseSteerArgs("SIM-123")).toThrow("Usage: /symphony:steer <ISSUE> <instruction>");
    expect(() => parseSteerArgs("   ")).toThrow("Usage: /symphony:steer <ISSUE> <instruction>");
  });
```

- [ ] **Step 2: Run command-args tests and verify they fail**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/command-args.test.ts
```

Expected: FAIL because `parseSteerArgs` is not defined.

- [ ] **Step 3: Implement steer argument parsing**

In `apps/symphony/pi-extension/src/command-args.ts`, add this interface after `AttachArgs`:

```ts
export interface SteerArgs {
  issueIdentifier: string;
  instruction: string;
}
```

Add this function after `parseAttachArgs`:

```ts
export function parseSteerArgs(args: string): SteerArgs {
  const trimmed = args.trim();
  const firstSpaceIndex = trimmed.search(/\s/);
  if (!trimmed || firstSpaceIndex === -1) throw new Error("Usage: /symphony:steer <ISSUE> <instruction>");

  const issueIdentifier = trimmed.slice(0, firstSpaceIndex).trim();
  const instruction = trimmed.slice(firstSpaceIndex).trim();
  if (!issueIdentifier || !instruction) throw new Error("Usage: /symphony:steer <ISSUE> <instruction>");
  return { issueIdentifier, instruction };
}
```

- [ ] **Step 4: Write failing command tests for refresh and steer**

Add these tests to `apps/symphony/pi-extension/src/commands.test.ts`:

```ts
  it("requests a manual refresh from the command", async () => {
    const runtime = new SymphonyRuntime();
    runtime.state.attachedBaseUrl = "http://127.0.0.1:8080";
    runtime.requestRefresh = vi.fn(async () => {
      runtime.state.lastKnownState = lastKnownState("http://127.0.0.1:8080");
      return {} as Awaited<ReturnType<SymphonyRuntime["requestRefresh"]>>;
    }) as SymphonyRuntime["requestRefresh"];

    const { commands, appendEntry } = registerCommands(runtime);
    const { ctx, notify } = commandContext();
    const refresh = commands.get("symphony:refresh");
    if (!refresh) throw new Error("expected refresh command");

    await refresh.handler("", ctx);

    expect(runtime.requestRefresh).toHaveBeenCalledOnce();
    expect(appendEntry).toHaveBeenCalledWith("symphony-extension-state", expect.objectContaining({ lastKnownState: runtime.state.lastKnownState }));
    expect(notify).toHaveBeenCalledWith("Symphony refresh requested; running 1, retry 0, blocked 0, completed 2", "info");
  });

  it("sends a steer instruction from the command", async () => {
    const runtime = new SymphonyRuntime();
    runtime.steerWorker = vi.fn(async () => ({ ok: true, issueId: "issue-123", issueIdentifier: "SIM-123", delivered: true, instructionPreview: "Use auth" })) as SymphonyRuntime["steerWorker"];

    const { commands, appendEntry } = registerCommands(runtime);
    const { ctx, notify } = commandContext();
    const steer = commands.get("symphony:steer");
    if (!steer) throw new Error("expected steer command");

    await steer.handler("SIM-123 Use auth", ctx);

    expect(runtime.steerWorker).toHaveBeenCalledWith("SIM-123", "Use auth");
    expect(appendEntry).toHaveBeenCalled();
    expect(notify).toHaveBeenCalledWith("Steer delivered to SIM-123: Use auth", "info");
  });
```

- [ ] **Step 5: Implement refresh and steer commands**

In `apps/symphony/pi-extension/src/commands.ts`, update the parser import:

```ts
import { parseAttachArgs, parseDoctorArgs, parseInitArgs, parseStartArgs, parseSteerArgs } from "./command-args.ts";
```

Add these command registrations after `symphony:status`:

```ts
  pi.registerCommand("symphony:refresh", {
    description: "Request an immediate Symphony poll refresh",
    handler: async (_args, ctx) => runCommandHandler(ctx, async () => {
      await runtime.requestRefresh();
      runtime.persist(pi);
      ctx.ui.notify(`Symphony refresh requested; ${runtimeCountsText(runtime)}`, "info");
    }),
  });

  pi.registerCommand("symphony:steer", {
    description: "Send an operator instruction to a running Symphony worker",
    handler: async (args, ctx) => runCommandHandler(ctx, async () => {
      const parsed = parseSteerArgs(args);
      const result = await runtime.steerWorker(parsed.issueIdentifier, parsed.instruction);
      runtime.persist(pi);
      ctx.ui.notify(`Steer delivered to ${result.issueIdentifier}: ${result.instructionPreview}`, "info");
    }),
  });
```

Add this helper near `helpText`:

```ts
function runtimeCountsText(runtime: SymphonyRuntime): string {
  const state = runtime.state.lastKnownState;
  if (!state) return "state unavailable";
  return `running ${state.runningCount}, retry ${state.retryCount}, blocked ${state.blockedCount}, completed ${state.completedCount}`;
}
```

Add these lines to the `Commands:` block in `helpText`:

```ts
    "/symphony:refresh",
    "/symphony:steer <ISSUE> <instruction>",
```

- [ ] **Step 6: Write failing tool tests for refresh and steer**

In `apps/symphony/pi-extension/src/tools.test.ts`, update the registered tool list assertion to include `symphony_refresh` and `symphony_steer`:

```ts
    expect([...tools.keys()].sort()).toEqual([
      "symphony_attach",
      "symphony_doctor",
      "symphony_help",
      "symphony_init",
      "symphony_refresh",
      "symphony_start",
      "symphony_status",
      "symphony_steer",
      "symphony_stop",
    ]);
```

Add these tests:

```ts
  it("requests a manual refresh from the tool", async () => {
    const runtime = new SymphonyRuntime();
    runtime.requestRefresh = vi.fn(async () => {
      runtime.state.lastKnownState = lastKnownState("http://127.0.0.1:8080");
      return {} as Awaited<ReturnType<SymphonyRuntime["requestRefresh"]>>;
    }) as SymphonyRuntime["requestRefresh"];
    const { tools, appendEntry } = registerTools(runtime);
    const refresh = tools.get("symphony_refresh");
    if (!refresh) throw new Error("expected refresh tool");

    const result = await refresh.execute("1", {}, new AbortController().signal, undefined, toolContext().ctx);

    expect(runtime.requestRefresh).toHaveBeenCalledOnce();
    expect(appendEntry).toHaveBeenCalled();
    expect(result).toMatchObject({
      content: [{ type: "text", text: "Symphony refresh requested" }],
      details: { state: runtime.state.lastKnownState },
    });
  });

  it("sends a steer instruction from the tool", async () => {
    const runtime = new SymphonyRuntime();
    runtime.steerWorker = vi.fn(async () => ({ ok: true, issueId: "issue-123", issueIdentifier: "SIM-123", delivered: true, instructionPreview: "Use auth" })) as SymphonyRuntime["steerWorker"];
    const { tools, appendEntry } = registerTools(runtime);
    const steer = tools.get("symphony_steer");
    if (!steer) throw new Error("expected steer tool");

    const result = await steer.execute("1", { issueIdentifier: "SIM-123", instruction: "Use auth" }, new AbortController().signal, undefined, toolContext().ctx);

    expect(runtime.steerWorker).toHaveBeenCalledWith("SIM-123", "Use auth", expect.any(AbortSignal));
    expect(appendEntry).toHaveBeenCalled();
    expect(result).toMatchObject({
      content: [{ type: "text", text: "Steer delivered to SIM-123: Use auth" }],
      details: { result: expect.objectContaining({ issueIdentifier: "SIM-123" }) },
    });
  });
```

- [ ] **Step 7: Implement refresh and steer tools**

In `apps/symphony/pi-extension/src/tools.ts`, update the help tool text:

```ts
      return toolOk("Symphony tools: symphony_init, symphony_doctor, symphony_start, symphony_attach, symphony_status, symphony_refresh, symphony_steer, symphony_stop, symphony_help", {
```

Add these tool registrations before `symphony_stop`:

```ts
  pi.registerTool(defineTool({
    name: "symphony_refresh",
    label: "Symphony Refresh",
    description: "Request an immediate Symphony poll refresh and return the updated health summary.",
    parameters: Type.Object({}),
    executionMode: SYMPHONY_TOOL_EXECUTION_MODE,
    async execute(_id, _params, signal) {
      try {
        await runtime.requestRefresh(signal);
        runtime.persist(pi);
        return toolOk("Symphony refresh requested", { state: runtime.state.lastKnownState });
      } catch (error) {
        throw new Error(formatError(error));
      }
    },
  }));

  pi.registerTool(defineTool({
    name: "symphony_steer",
    label: "Symphony Steer",
    description: "Send an operator instruction to a running Symphony worker.",
    parameters: Type.Object({ issueIdentifier: Type.String(), instruction: Type.String() }),
    executionMode: SYMPHONY_TOOL_EXECUTION_MODE,
    async execute(_id, params, signal) {
      try {
        const result = await runtime.steerWorker(params.issueIdentifier, params.instruction, signal);
        runtime.persist(pi);
        return toolOk(`Steer delivered to ${result.issueIdentifier}: ${result.instructionPreview}`, { result, state: runtime.state.lastKnownState });
      } catch (error) {
        throw new Error(formatError(error));
      }
    },
  }));
```

- [ ] **Step 8: Update README command and key documentation**

In `apps/symphony/pi-extension/README.md`, replace the Slice 1 command section with:

```md
## Commands through Slice 2

- `/symphony:help`
- `/symphony:init [--force]`
- `/symphony:doctor [workflow]`
- `/symphony:start [workflow]`
- `/symphony:attach <url>`
- `/symphony:dashboard`
- `/symphony:status`
- `/symphony:refresh`
- `/symphony:steer <ISSUE> <instruction>`
- `/symphony:stop`

## Dashboard keys through Slice 2

- `↑` / `↓` selects a running worker.
- `r` requests an immediate Symphony refresh and reloads state.
- `s` prompts for a steer instruction for the selected worker.
- `d` toggles selected-worker details.
- `q` or Escape closes the dashboard and leaves Symphony running.
```

- [ ] **Step 9: Run command and tool tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test src/command-args.test.ts src/commands.test.ts src/tools.test.ts
```

Expected: PASS for command parsing, refresh command/tool, steer command/tool, and existing Slice 1 command/tool tests.

- [ ] **Step 10: Commit command and tool changes**

```bash
git add apps/symphony/pi-extension/src/command-args.ts apps/symphony/pi-extension/src/command-args.test.ts apps/symphony/pi-extension/src/commands.ts apps/symphony/pi-extension/src/commands.test.ts apps/symphony/pi-extension/src/tools.ts apps/symphony/pi-extension/src/tools.test.ts apps/symphony/pi-extension/README.md
git commit -m "feat: expose Symphony refresh and steer controls"
```

---

### Task 6: Slice 2 validation pass

**Files:**
- Verify: `apps/symphony/pi-extension/src/*.ts`
- Verify: `apps/symphony/pi-extension/package.json`
- Verify: `apps/symphony/pi-extension/README.md`

- [ ] **Step 1: Run package tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test -- --runInBand
```

Expected: PASS for all pi-extension Vitest tests.

- [ ] **Step 2: Run package typecheck**

Run:

```bash
pnpm --dir apps/symphony/pi-extension typecheck
```

Expected: PASS with no TypeScript errors.

- [ ] **Step 3: Run package lint**

Run:

```bash
pnpm --dir apps/symphony/pi-extension lint
```

Expected: PASS with zero ESLint warnings.

- [ ] **Step 4: Run affected repo validation**

Run:

```bash
pnpm run validate:affected
```

Expected: PASS for affected lint, typecheck, and test tasks.

- [ ] **Step 5: Check Slice 2 acceptance manually in the code**

Verify these exact conditions from the changed files:

```text
apps/symphony/pi-extension/src/dashboard.ts renders "Running workers".
apps/symphony/pi-extension/src/dashboard.ts renders "Selected worker" details with issue, tracker state, attempt, turns, last activity, worker host, workspace, and error.
apps/symphony/pi-extension/src/dashboard.ts maps r to runtime.requestRefresh through the refresh option.
apps/symphony/pi-extension/src/dashboard.ts maps s to prompt plus runtime.steerWorker through the steer option.
apps/symphony/pi-extension/src/event-stream.ts connects to /api/v1/events.
apps/symphony/pi-extension/src/runtime.ts retains recent worker/runtime events.
apps/symphony/pi-extension/src/commands.ts registers symphony:refresh and symphony:steer.
apps/symphony/pi-extension/src/tools.ts registers symphony_refresh and symphony_steer.
```

- [ ] **Step 6: Commit validation fixes if any files changed during validation**

If validation required edits, run:

```bash
git add apps/symphony/pi-extension
git commit -m "fix: stabilize Symphony worker operations extension"
```

Expected: Create this commit only when validation edits were made.

---

## Self-review

Spec coverage:

- Running workers table: Task 3 renders `Running workers` from `buildWorkerRows` and tests selected rows.
- Selected-worker details: Task 3 renders issue, tracker state, attempt, turns, last activity, worker host, workspace, and error preview.
- Manual refresh: Task 1 adds `POST /api/v1/refresh`; Task 2 adds `runtime.requestRefresh`; Task 3 maps `r`; Task 5 exposes `/symphony:refresh` and `symphony_refresh`.
- Dashboard steering: Task 1 adds `POST /api/v1/steer`; Task 2 adds `runtime.steerWorker`; Task 3 maps `s` to prompt and steer.
- Recent worker/runtime events: Task 4 connects to `/api/v1/events`, records worker/runtime events in runtime, and renders them in the dashboard.

Placeholder scan:

- No task uses placeholder language or unspecific testing instructions.
- Each code-changing step includes exact code blocks or exact replacement text.

Type consistency:

- HTTP client returns camelCase `RefreshResponse` and `SteerResponse`; runtime and command/tool tests use those names.
- Dashboard model reads snake_case Symphony API fields and returns display-ready strings for the TUI.
- Dashboard uses `runtime.lastState` and `runtime.recentEvents`; runtime defines both fields in Task 2.
