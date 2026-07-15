---
type: Plan
title: Wave 3 Symphony Console Escalations
description: Archived implementation plan: Wave 3 Symphony Console Escalations.
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---

# Wave 3 Symphony Console Escalations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend `@kata-sh/pi-symphony-extension` so the Pi console renders retry, blocked, completed, and pending escalation state, supports responding to escalations, and reflects escalation lifecycle events.

**Architecture:** Keep Wave 3 in the existing pi-extension package. Add typed HTTP contracts to `http-client.ts`, keep view-model transformation in `console-model.ts`, keep interaction/rendering in `console.ts`, and expose escalation response through `SymphonyRuntime` so UI code does not call the client directly.

**Tech Stack:** TypeScript, Vitest, Pi extension APIs, `@earendil-works/pi-tui`, Symphony HTTP API.

---

## Scope

Implements Wave 3 from `docs/superpowers/specs/2026-05-14-pi-symphony-extension-design.md`:

- Slice 3: render retry queue, blocked issues, completed issues, and a detail panel for running, retry, blocked, and completed states.
- Slice 4: render pending escalations, support dashboard responses, and show escalation lifecycle events.

## File Structure

- Modify `apps/symphony/pi-extension/src/http-client.ts`
  - Owns TypeScript API contracts and response validation for `GET /api/v1/state`, `GET /api/v1/escalations`, and `POST /api/v1/escalations/:request_id/respond`.
- Modify `apps/symphony/pi-extension/src/http-client.test.ts`
  - Mock HTTP tests for typed state entries, escalation list, escalation response, and API errors.
- Modify `apps/symphony/pi-extension/src/runtime.ts`
  - Adds `respondToEscalation()` and records escalation lifecycle events in recent activity.
- Modify `apps/symphony/pi-extension/src/runtime.test.ts`
  - Verifies response dispatch, refresh after response, and recent escalation events.
- Modify `apps/symphony/pi-extension/src/console-model.ts`
  - Builds issue rows for running, retry, blocked, and completed buckets; builds escalation rows; formats escalation lifecycle events.
- Modify `apps/symphony/pi-extension/src/console-model.test.ts`
  - Unit tests for row projection, detail fields, sorting, and escalation event formatting.
- Modify `apps/symphony/pi-extension/src/console.ts`
  - Renders the new sections, tracks a single linear selection across issue rows and escalation rows, responds to escalations, and keeps steering limited to selected running workers.
- Modify `apps/symphony/pi-extension/src/console.test.ts`
  - TUI rendering and key-handling tests for Wave 3.
- Modify `apps/symphony/pi-extension/src/commands.ts`
  - Updates console shortcut registrations and help text to include escalation response controls.
- Modify `apps/symphony/pi-extension/src/commands.test.ts`
  - Verifies shortcut registration and descriptions.
- Modify `apps/symphony/pi-extension/README.md`
  - Documents Wave 3 console keys and manual verification.

## API shapes to preserve

Use these Symphony HTTP fields already emitted by `apps/symphony/src/domain.rs` and `apps/symphony/src/http_server.rs`:

```ts
interface RetryQueueEntryResponse {
  issue_id: string;
  identifier: string;
  attempt: number;
  due_in_ms: number;
  error?: string | null;
  worker_host?: string | null;
  workspace_path?: string | null;
}

interface BlockedIssueResponse {
  issue_id: string;
  identifier: string;
  title: string;
  state: string;
  blocker_identifiers: string[];
}

interface CompletedIssueResponse {
  issue_id: string;
  identifier: string;
  title: string;
  completed_at?: string | null;
  issue_url?: string | null;
}

interface PendingEscalationResponse {
  request_id: string;
  issue_id: string;
  issue_identifier: string;
  method: string;
  preview: string;
  created_at: string;
  timeout_ms: number;
}
```

---

### Task 1: Add typed Wave 3 HTTP contracts

**Files:**
- Modify: `apps/symphony/pi-extension/src/http-client.ts`
- Test: `apps/symphony/pi-extension/src/http-client.test.ts`

- [ ] **Step 1: Write failing HTTP client tests**

Append these tests inside the existing `describe("SymphonyHttpClient", () => { ... })` block in `apps/symphony/pi-extension/src/http-client.test.ts`:

```ts
  it("fetches typed Wave 3 state entries", async () => {
    const baseUrl = await serve((req) => {
      expect(req.url).toBe("/api/v1/state");
      return {
        status: 200,
        body: validState({
          retry_queue: [{ issue_id: "issue-retry", identifier: "SIM-2", attempt: 3, due_in_ms: 45000, error: "rate limit", worker_host: "host-a", workspace_path: "/tmp/retry" }],
          blocked: [{ issue_id: "issue-blocked", identifier: "SIM-3", title: "Blocked work", state: "Todo", blocker_identifiers: ["SIM-1"] }],
          pending_escalations: [{ request_id: "esc-1", issue_id: "issue-running", issue_identifier: "SIM-1", method: "approval", preview: "Approve command?", created_at: "2026-05-14T12:00:00Z", timeout_ms: 600000 }],
          completed: [{ issue_id: "issue-done", identifier: "SIM-4", title: "Done work", completed_at: "2026-05-14T13:00:00Z" }],
        }),
      };
    });

    const client = new SymphonyHttpClient(baseUrl);
    const state = await client.getState();

    expect(state.retry_queue[0]).toMatchObject({ identifier: "SIM-2", attempt: 3, due_in_ms: 45000 });
    expect(state.blocked[0]).toMatchObject({ identifier: "SIM-3", blocker_identifiers: ["SIM-1"] });
    expect(state.pending_escalations[0]).toMatchObject({ request_id: "esc-1", issue_identifier: "SIM-1" });
    expect(state.completed[0]).toMatchObject({ identifier: "SIM-4", completed_at: "2026-05-14T13:00:00Z" });
  });

  it("fetches pending escalations", async () => {
    const baseUrl = await serve((req) => {
      expect(req.method).toBe("GET");
      expect(req.url).toBe("/api/v1/escalations");
      return {
        status: 200,
        body: {
          pending: [{ request_id: "esc-1", issue_id: "issue-running", issue_identifier: "SIM-1", method: "approval", preview: "Approve command?", created_at: "2026-05-14T12:00:00Z", timeout_ms: 600000 }],
        },
      };
    });

    const client = new SymphonyHttpClient(baseUrl);

    await expect(client.getEscalations()).resolves.toEqual({
      pending: [{ request_id: "esc-1", issue_id: "issue-running", issue_identifier: "SIM-1", method: "approval", preview: "Approve command?", created_at: "2026-05-14T12:00:00Z", timeout_ms: 600000 }],
    });
  });

  it("responds to a pending escalation", async () => {
    const baseUrl = await serve((req, body) => {
      expect(req.method).toBe("POST");
      expect(req.url).toBe("/api/v1/escalations/esc-1/respond");
      expect(JSON.parse(body)).toEqual({ response: { approved: true }, responder_id: "pi-dashboard" });
      return { status: 200, body: { ok: true } };
    });

    const client = new SymphonyHttpClient(baseUrl);

    await expect(client.respondEscalation("esc-1", { approved: true }, "pi-dashboard")).resolves.toEqual({ ok: true });
  });

  it.each([
    ["missing escalation", 404, { error: "escalation_not_found" }, "escalation_not_found"],
    ["already resolved escalation", 409, { error: "escalation_already_resolved" }, "escalation_already_resolved"],
  ])("normalizes escalation response errors: %s", async (_name, status, body, code) => {
    const baseUrl = await serve(() => ({ status, body }));
    const client = new SymphonyHttpClient(baseUrl);

    await expect(client.respondEscalation("esc-1", "yes", "pi-dashboard")).rejects.toMatchObject({
      kind: "api_error",
      message: code,
      details: expect.objectContaining({ code, status }),
    } satisfies Partial<SymphonyExtensionError>);
  });

  it.each([
    ["retry_queue entry", validState({ retry_queue: [{ identifier: "SIM-2" }] }), "retry_queue.0.issue_id"],
    ["blocked entry", validState({ blocked: [{ issue_id: "issue-blocked", identifier: "SIM-3" }] }), "blocked.0.title"],
    ["pending escalation", validState({ pending_escalations: [{ request_id: "esc-1" }] }), "pending_escalations.0.issue_id"],
    ["completed entry", validState({ completed: [{ issue_id: "issue-done", identifier: "SIM-4" }] }), "completed.0.title"],
  ])("rejects malformed Wave 3 state field: %s", async (_name, body, field) => {
    const baseUrl = await serve(() => ({ status: 200, body }));
    const client = new SymphonyHttpClient(baseUrl);

    await expect(client.getState()).rejects.toMatchObject({
      kind: "non_symphony_response",
      details: expect.objectContaining({ field }),
    } satisfies Partial<SymphonyExtensionError>);
  });
```

- [ ] **Step 2: Run the failing HTTP client tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/http-client.test.ts
```

Expected: FAIL because `getEscalations`, `respondEscalation`, and typed Wave 3 state validators do not exist yet.

- [ ] **Step 3: Add response interfaces and client methods**

In `apps/symphony/pi-extension/src/http-client.ts`, replace the current untyped `retry_queue`, `blocked`, and `completed` fields with these exported interfaces and fields:

```ts
export interface RetryQueueEntryResponse {
  issue_id: string;
  identifier: string;
  attempt: number;
  due_in_ms: number;
  error?: string | null;
  worker_host?: string | null;
  workspace_path?: string | null;
}

export interface BlockedIssueResponse {
  issue_id: string;
  identifier: string;
  title: string;
  state: string;
  blocker_identifiers: string[];
}

export interface CompletedIssueResponse {
  issue_id: string;
  identifier: string;
  title: string;
  completed_at?: string | null;
  issue_url?: string | null;
}

export interface PendingEscalationResponse {
  request_id: string;
  issue_id: string;
  issue_identifier: string;
  method: string;
  preview: string;
  created_at: string;
  timeout_ms: number;
}

export interface EscalationListResponse {
  pending: PendingEscalationResponse[];
}

export interface EscalationRespondResponse {
  ok: boolean;
}

export interface SymphonyStateResponse {
  tracker_project_url?: string | null;
  running?: Record<string, RunAttemptResponse>;
  running_sessions?: Record<string, RunningSessionSnapshotResponse>;
  running_session_info?: Record<string, WorkerSessionInfoResponse>;
  retry_queue: RetryQueueEntryResponse[];
  blocked: BlockedIssueResponse[];
  pending_escalations?: PendingEscalationResponse[];
  completed: CompletedIssueResponse[];
  polling?: {
    checking?: boolean;
    next_poll_in_ms?: number;
    poll_interval_ms?: number;
    poll_count?: number;
    last_poll_at?: string | null;
  };
}
```

Add these methods to `SymphonyHttpClient` after `refresh()` and before `steer()`:

```ts
  async getEscalations(signal?: AbortSignal): Promise<EscalationListResponse> {
    const path = "/api/v1/escalations";
    const json = await this.requestJson(path, { method: "GET", signal });
    return validateEscalationListResponse(json, { baseUrl: this.baseUrl, path });
  }

  async respondEscalation(requestId: string, response: unknown, responderId = "pi-dashboard", signal?: AbortSignal): Promise<EscalationRespondResponse> {
    const path = `/api/v1/escalations/${encodeURIComponent(requestId)}/respond`;
    const json = await this.requestJson(path, {
      method: "POST",
      signal,
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ response, responder_id: responderId }),
    });
    return validateEscalationRespondResponse(json, { baseUrl: this.baseUrl, path, requestId });
  }
```

- [ ] **Step 4: Validate Wave 3 state arrays**

In `validateSymphonyStateResponse()`, replace the three optional array validations and the pending escalation optional validation with typed validators:

```ts
  validateRetryQueue(value.retry_queue, details);
  validateBlockedIssues(value.blocked, details);
  validateCompletedIssues(value.completed, details);
  validatePendingEscalations(value.pending_escalations, details);
```

Add these validation functions near `validateRunningAttempts()`:

```ts
function validateRetryQueue(value: unknown, details: Record<string, unknown>): void {
  validateOptionalArray({ retry_queue: value }, "retry_queue", details);
  if (value === undefined) return;
  for (const [index, entry] of value.entries()) {
    const field = `retry_queue.${index}`;
    if (!isRecord(entry)) throwNonSymphonyState(details, "state response field had an invalid shape", { field, expected: "object" });
    validateRequiredString(entry, "issue_id", details, `${field}.issue_id`);
    validateRequiredString(entry, "identifier", details, `${field}.identifier`);
    validateRequiredNumber(entry, "attempt", details, `${field}.attempt`);
    validateRequiredNumber(entry, "due_in_ms", details, `${field}.due_in_ms`);
    validateOptionalStringOrNull(entry, "error", details, `${field}.error`);
    validateOptionalStringOrNull(entry, "worker_host", details, `${field}.worker_host`);
    validateOptionalStringOrNull(entry, "workspace_path", details, `${field}.workspace_path`);
  }
}

function validateBlockedIssues(value: unknown, details: Record<string, unknown>): void {
  validateOptionalArray({ blocked: value }, "blocked", details);
  if (value === undefined) return;
  for (const [index, entry] of value.entries()) {
    const field = `blocked.${index}`;
    if (!isRecord(entry)) throwNonSymphonyState(details, "state response field had an invalid shape", { field, expected: "object" });
    validateRequiredString(entry, "issue_id", details, `${field}.issue_id`);
    validateRequiredString(entry, "identifier", details, `${field}.identifier`);
    validateRequiredString(entry, "title", details, `${field}.title`);
    validateRequiredString(entry, "state", details, `${field}.state`);
    validateStringArray(entry.blocker_identifiers, details, `${field}.blocker_identifiers`);
  }
}

function validateCompletedIssues(value: unknown, details: Record<string, unknown>): void {
  validateOptionalArray({ completed: value }, "completed", details);
  if (value === undefined) return;
  for (const [index, entry] of value.entries()) {
    const field = `completed.${index}`;
    if (!isRecord(entry)) throwNonSymphonyState(details, "state response field had an invalid shape", { field, expected: "object" });
    validateRequiredString(entry, "issue_id", details, `${field}.issue_id`);
    validateRequiredString(entry, "identifier", details, `${field}.identifier`);
    validateRequiredString(entry, "title", details, `${field}.title`);
    validateOptionalStringOrNull(entry, "completed_at", details, `${field}.completed_at`);
    validateOptionalStringOrNull(entry, "issue_url", details, `${field}.issue_url`);
  }
}

function validatePendingEscalations(value: unknown, details: Record<string, unknown>): void {
  if (value === undefined) return;
  validateOptionalArray({ pending_escalations: value }, "pending_escalations", details);
  for (const [index, entry] of value.entries()) {
    validatePendingEscalation(entry, details, `pending_escalations.${index}`);
  }
}

function validatePendingEscalation(value: unknown, details: Record<string, unknown>, field: string): void {
  if (!isRecord(value)) throwNonSymphonyState(details, "state response field had an invalid shape", { field, expected: "object" });
  validateRequiredString(value, "request_id", details, `${field}.request_id`);
  validateRequiredString(value, "issue_id", details, `${field}.issue_id`);
  validateRequiredString(value, "issue_identifier", details, `${field}.issue_identifier`);
  validateRequiredString(value, "method", details, `${field}.method`);
  validateRequiredString(value, "preview", details, `${field}.preview`);
  validateRequiredString(value, "created_at", details, `${field}.created_at`);
  validateRequiredNumber(value, "timeout_ms", details, `${field}.timeout_ms`);
}

function validateStringArray(value: unknown, details: Record<string, unknown>, field: string): void {
  if (!Array.isArray(value) || value.some((item) => typeof item !== "string")) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field, expected: "string[]" });
  }
}
```

Add escalation endpoint validators near `validateSteerResponse()`:

```ts
function validateEscalationListResponse(value: unknown, details: Record<string, unknown>): EscalationListResponse {
  if (!isRecord(value)) {
    throwNonSymphonyEscalations(details, "escalations response was not an object");
  }
  if (!Array.isArray(value.pending)) {
    throwNonSymphonyEscalations(details, "escalations response field had an invalid shape", { field: "pending", expected: "array" });
  }
  for (const [index, entry] of value.pending.entries()) {
    validatePendingEscalation(entry, details, `pending.${index}`);
  }
  return { pending: value.pending as PendingEscalationResponse[] };
}

function validateEscalationRespondResponse(value: unknown, details: Record<string, unknown>): EscalationRespondResponse {
  if (!isRecord(value)) {
    throwNonSymphonyEscalations(details, "escalation response was not an object");
  }
  if (typeof value.ok !== "boolean") {
    throwNonSymphonyEscalations(details, "escalation response field had an invalid shape", { field: "ok", expected: "boolean" });
  }
  return { ok: value.ok };
}

function throwNonSymphonyEscalations(details: Record<string, unknown>, reason: string, extraDetails: Record<string, unknown> = {}): never {
  throw new SymphonyExtensionError("non_symphony_response", "Response did not look like Symphony escalation response", {
    ...details,
    reason,
    ...extraDetails,
  });
}
```

- [ ] **Step 5: Normalize simple escalation error bodies**

In `requestJson()`, replace the `if (!response.ok) { ... }` block with:

```ts
    if (!response.ok) {
      const envelope = parseApiErrorEnvelope(json);
      if (envelope?.error?.message) {
        throw new SymphonyExtensionError("api_error", envelope.error.message, {
          url,
          status: response.status,
          code: envelope.error.code,
          details: envelope.error.details,
        });
      }
      if (isRecord(json) && typeof json.error === "string") {
        throw new SymphonyExtensionError("api_error", json.error, {
          url,
          status: response.status,
          code: json.error,
        });
      }
      throw new SymphonyExtensionError("non_symphony_response", "Symphony HTTP API returned an unexpected error response", {
        url,
        status: response.status,
        body: json,
      });
    }
```

- [ ] **Step 6: Run HTTP client tests and commit**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/http-client.test.ts
```

Expected: PASS.

Commit:

```bash
git add apps/symphony/pi-extension/src/http-client.ts apps/symphony/pi-extension/src/http-client.test.ts
git commit -m "feat(pi-symphony): add wave 3 http contracts"
```

---

### Task 2: Build Wave 3 console view models

**Files:**
- Modify: `apps/symphony/pi-extension/src/console-model.ts`
- Test: `apps/symphony/pi-extension/src/console-model.test.ts`

- [ ] **Step 1: Write failing console-model tests**

Update the import in `apps/symphony/pi-extension/src/console-model.test.ts`:

```ts
import { buildEscalationRows, buildIssueRows, buildWorkerRows, formatEventRows } from "./console-model.ts";
```

Add this test after `builds sorted worker rows with selected-worker detail fields`:

```ts
  it("builds issue rows for running, retry, blocked, and completed buckets", () => {
    const state: SymphonyStateResponse = {
      ...stateFixture(),
      retry_queue: [{ issue_id: "issue-retry", identifier: "SIM-200", attempt: 3, due_in_ms: 90000, error: "rate limit", worker_host: "host-b", workspace_path: "/tmp/retry" }],
      blocked: [{ issue_id: "issue-blocked", identifier: "SIM-300", title: "Blocked work", state: "Todo", blocker_identifiers: ["SIM-100", "SIM-101"] }],
      completed: [{ issue_id: "issue-done", identifier: "SIM-400", title: "Done work", completed_at: "2026-05-14T13:00:00Z" }],
    };

    const rows = buildIssueRows(state);

    expect(rows.map((row) => `${row.kind}:${row.issueIdentifier}`)).toEqual([
      "running:SIM-123",
      "running:SIM-777",
      "retry:SIM-200",
      "blocked:SIM-300",
      "completed:SIM-400",
    ]);
    expect(rows.find((row) => row.kind === "retry")).toMatchObject({
      issueIdentifier: "SIM-200",
      title: "rate limit",
      status: "retry in 1m 30s",
      attempt: "3",
      workerHost: "host-b",
      workspacePath: "/tmp/retry",
      errorPreview: "rate limit",
    });
    expect(rows.find((row) => row.kind === "blocked")).toMatchObject({
      issueIdentifier: "SIM-300",
      title: "Blocked work",
      status: "Todo",
      blockers: "SIM-100, SIM-101",
    });
    expect(rows.find((row) => row.kind === "completed")).toMatchObject({
      issueIdentifier: "SIM-400",
      title: "Done work",
      status: "completed",
      completedAt: "2026-05-14T13:00:00Z",
    });
  });

  it("builds pending escalation rows sorted by creation time", () => {
    const state: SymphonyStateResponse = {
      ...stateFixture(),
      pending_escalations: [
        { request_id: "esc-2", issue_id: "issue-777", issue_identifier: "SIM-777", method: "input", preview: "Need details", created_at: "2026-05-14T12:02:00Z", timeout_ms: 300000 },
        { request_id: "esc-1", issue_id: "issue-123", issue_identifier: "SIM-123", method: "approval", preview: "Approve command", created_at: "2026-05-14T12:01:00Z", timeout_ms: 600000 },
      ],
    };

    const rows = buildEscalationRows(state);

    expect(rows.map((row) => row.requestId)).toEqual(["esc-1", "esc-2"]);
    expect(rows[0]).toMatchObject({ issueIdentifier: "SIM-123", method: "approval", preview: "Approve command", timeout: "10m 0s" });
  });
```

Add this escalation event formatting test after the existing event tests:

```ts
  it("formats escalation lifecycle events", () => {
    const events: SymphonyEventEnvelope[] = [
      { version: "v1", sequence: 1, timestamp: "2026-05-14T12:00:00Z", kind: "escalation_created", severity: "info", issue: "SIM-123", event: "escalation_created", payload: { summary: "SIM-123 needs input: approval" } },
      { version: "v1", sequence: 2, timestamp: "2026-05-14T12:01:00Z", kind: "escalation_responded", severity: "info", issue: "SIM-123", event: "escalation_responded", payload: { summary: "request esc-1 responded by pi-dashboard in 300ms" } },
      { version: "v1", sequence: 3, timestamp: "2026-05-14T12:02:00Z", kind: "escalation_timed_out", severity: "warn", issue: "SIM-456", event: "escalation_timed_out", payload: { summary: "request esc-2 timed out after 600000ms" } },
    ];

    expect(formatEventRows(events)).toEqual([
      "2026-05-14T12:02:00Z warn escalation_timed_out SIM-456 escalation_timed_out request esc-2 timed out after 600000ms",
      "2026-05-14T12:01:00Z info escalation_responded SIM-123 escalation_responded request esc-1 responded by pi-dashboard in 300ms",
      "2026-05-14T12:00:00Z info escalation_created SIM-123 escalation_created SIM-123 needs input: approval",
    ]);
  });
```

- [ ] **Step 2: Run the failing console-model tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/console-model.test.ts
```

Expected: FAIL because `buildIssueRows`, `buildEscalationRows`, and escalation event filtering do not exist yet.

- [ ] **Step 3: Add issue and escalation row interfaces**

In `apps/symphony/pi-extension/src/console-model.ts`, replace the first import with:

```ts
import type { PendingEscalationResponse, RunAttemptResponse, SymphonyEventEnvelope, SymphonyStateResponse } from "./http-client.ts";
```

Add these interfaces after `WorkerRow`:

```ts
export type IssueRowKind = "running" | "retry" | "blocked" | "completed";

export interface IssueRow {
  kind: IssueRowKind;
  issueId: string;
  issueIdentifier: string;
  title: string;
  status: string;
  trackerState: string;
  attempt: string;
  turnCount: string;
  maxTurns: string;
  lastActivity: string;
  workerHost: string;
  workspacePath: string;
  errorPreview: string;
  blockers: string;
  completedAt: string;
}

export interface EscalationRow {
  requestId: string;
  issueId: string;
  issueIdentifier: string;
  method: string;
  preview: string;
  createdAt: string;
  timeout: string;
}
```

- [ ] **Step 4: Add issue row builders**

Add these functions after `buildWorkerRows()`:

```ts
export function buildIssueRows(state: SymphonyStateResponse | undefined): IssueRow[] {
  if (!state) return [];
  return [
    ...buildWorkerRows(state).map(workerToIssueRow),
    ...state.retry_queue.map(retry => ({
      kind: "retry" as const,
      issueId: retry.issue_id,
      issueIdentifier: retry.identifier,
      title: retry.error ?? "pending retry",
      status: `retry in ${formatDuration(retry.due_in_ms)}`,
      trackerState: "retry",
      attempt: String(retry.attempt),
      turnCount: "-",
      maxTurns: "-",
      lastActivity: "-",
      workerHost: retry.worker_host ?? "local",
      workspacePath: retry.workspace_path ?? "-",
      errorPreview: retry.error ? truncateText(retry.error, 120) : "-",
      blockers: "-",
      completedAt: "-",
    })).sort((left, right) => left.issueIdentifier.localeCompare(right.issueIdentifier)),
    ...state.blocked.map(blocked => ({
      kind: "blocked" as const,
      issueId: blocked.issue_id,
      issueIdentifier: blocked.identifier,
      title: blocked.title,
      status: blocked.state,
      trackerState: blocked.state,
      attempt: "-",
      turnCount: "-",
      maxTurns: "-",
      lastActivity: "-",
      workerHost: "-",
      workspacePath: "-",
      errorPreview: "-",
      blockers: blocked.blocker_identifiers.length > 0 ? blocked.blocker_identifiers.join(", ") : "-",
      completedAt: "-",
    })).sort((left, right) => left.issueIdentifier.localeCompare(right.issueIdentifier)),
    ...state.completed.map(completed => ({
      kind: "completed" as const,
      issueId: completed.issue_id,
      issueIdentifier: completed.identifier,
      title: completed.title,
      status: "completed",
      trackerState: "completed",
      attempt: "-",
      turnCount: "-",
      maxTurns: "-",
      lastActivity: completed.completed_at ?? "-",
      workerHost: "-",
      workspacePath: "-",
      errorPreview: "-",
      blockers: "-",
      completedAt: completed.completed_at ?? "-",
    })).sort((left, right) => left.issueIdentifier.localeCompare(right.issueIdentifier)),
  ];
}

function workerToIssueRow(worker: WorkerRow): IssueRow {
  return {
    kind: "running",
    issueId: worker.issueId,
    issueIdentifier: worker.issueIdentifier,
    title: worker.title,
    status: worker.status,
    trackerState: worker.trackerState,
    attempt: worker.attempt,
    turnCount: worker.turnCount,
    maxTurns: worker.maxTurns,
    lastActivity: worker.lastActivity,
    workerHost: worker.workerHost,
    workspacePath: worker.workspacePath,
    errorPreview: worker.errorPreview,
    blockers: "-",
    completedAt: "-",
  };
}
```

- [ ] **Step 5: Add escalation row builders and duration formatting**

Add these functions after `buildIssueRows()`:

```ts
export function buildEscalationRows(state: SymphonyStateResponse | undefined): EscalationRow[] {
  return [...(state?.pending_escalations ?? [])]
    .sort(compareEscalations)
    .map(escalation => ({
      requestId: escalation.request_id,
      issueId: escalation.issue_id,
      issueIdentifier: escalation.issue_identifier,
      method: escalation.method,
      preview: truncateText(escalation.preview, 120),
      createdAt: escalation.created_at,
      timeout: formatDuration(escalation.timeout_ms),
    }));
}

function compareEscalations(left: PendingEscalationResponse, right: PendingEscalationResponse): number {
  const leftTime = Date.parse(left.created_at);
  const rightTime = Date.parse(right.created_at);
  if (Number.isFinite(leftTime) && Number.isFinite(rightTime) && leftTime !== rightTime) return leftTime - rightTime;
  return left.request_id.localeCompare(right.request_id);
}

function formatDuration(milliseconds: number): string {
  if (!Number.isFinite(milliseconds) || milliseconds <= 0) return "0s";
  const totalSeconds = Math.ceil(milliseconds / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes === 0) return `${seconds}s`;
  return `${minutes}m ${seconds}s`;
}
```

- [ ] **Step 6: Include escalation lifecycle events in recent activity**

In `formatEventRows()`, replace the current filter with:

```ts
    .filter((event) => event.kind === "worker" || event.kind === "runtime" || event.kind.startsWith("escalation_"))
```

- [ ] **Step 7: Run console-model tests and commit**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/console-model.test.ts
```

Expected: PASS.

Commit:

```bash
git add apps/symphony/pi-extension/src/console-model.ts apps/symphony/pi-extension/src/console-model.test.ts
git commit -m "feat(pi-symphony): model wave 3 console rows"
```

---

### Task 3: Render retry, blocked, completed, and issue details

**Files:**
- Modify: `apps/symphony/pi-extension/src/console.ts`
- Test: `apps/symphony/pi-extension/src/console.test.ts`

- [ ] **Step 1: Write failing console rendering tests**

In `apps/symphony/pi-extension/src/console.test.ts`, update `workerStateFixture()` so `retry_queue`, `blocked`, and `completed` contain Wave 3 entries:

```ts
    retry_queue: [{ issue_id: "issue-retry", identifier: "SIM-200", attempt: 3, due_in_ms: 90000, error: "rate limit", worker_host: "host-b", workspace_path: "/tmp/retry" }],
    blocked: [{ issue_id: "issue-blocked", identifier: "SIM-300", title: "Blocked work", state: "Todo", blocker_identifiers: ["SIM-100", "SIM-101"] }],
    completed: [{ issue_id: "issue-done", identifier: "SIM-400", title: "Done work", completed_at: "2026-05-14T13:00:00Z" }],
```

Add this test after `renders running workers, selected-worker details, and recent runtime events`:

```ts
  it("renders retry, blocked, completed, and selected issue details", () => {
    const state = createDefaultState();
    state.console.showDetails = true;
    const consoleComponent = new SymphonyConsoleComponent({
      state,
      getState: () => workerStateFixture(),
      getEvents: () => [],
      refresh: async () => undefined,
      steer: async () => undefined,
      respondToEscalation: async () => undefined,
      prompt: async () => undefined,
      close: () => undefined,
      requestRender: () => undefined,
      notify: () => undefined,
    });

    let output = consoleComponent.render(180).join("\n");
    expect(output).toContain("Retry Queue");
    expect(output).toContain("SIM-200");
    expect(output).toContain("retry in 1m 30s");
    expect(output).toContain("Blocked Issues");
    expect(output).toContain("SIM-300");
    expect(output).toContain("SIM-100, SIM-101");
    expect(output).toContain("Completed Issues");
    expect(output).toContain("SIM-400");
    expect(output).toContain("2026-05-14T13:00:00Z");
    expect(output).toContain("Selected Issue");
    expect(output).toContain("kind: running");

    consoleComponent.handleInput("\u001b[B");
    consoleComponent.handleInput("\u001b[B");
    output = consoleComponent.render(180).join("\n");
    expect(output).toContain("issue: SIM-200 rate limit");
    expect(output).toContain("kind: retry");
    expect(output).toContain("status: retry in 1m 30s");
    expect(output).toContain("workspace: /tmp/retry");

    consoleComponent.handleInput("\u001b[B");
    output = consoleComponent.render(180).join("\n");
    expect(output).toContain("issue: SIM-300 Blocked work");
    expect(output).toContain("kind: blocked");
    expect(output).toContain("blockers: SIM-100, SIM-101");

    consoleComponent.handleInput("\u001b[B");
    output = consoleComponent.render(180).join("\n");
    expect(output).toContain("issue: SIM-400 Done work");
    expect(output).toContain("kind: completed");
    expect(output).toContain("completed at: 2026-05-14T13:00:00Z");
  });

  it("only steers when a running worker is selected", async () => {
    const notify = vi.fn();
    const steer = vi.fn(async () => undefined);
    const consoleComponent = new SymphonyConsoleComponent({
      state: createDefaultState(),
      getState: () => workerStateFixture(),
      getEvents: () => [],
      refresh: async () => undefined,
      steer,
      respondToEscalation: async () => undefined,
      prompt: async () => "Use auth",
      close: () => undefined,
      requestRender: () => undefined,
      notify,
    });

    consoleComponent.handleInput("\u001b[B");
    consoleComponent.handleInput("\u001b[B");
    consoleComponent.handleInput("s");
    await expect.poll(() => notify.mock.calls.length, { interval: 10, timeout: 1000 }).toBe(1);

    expect(steer).not.toHaveBeenCalled();
    expect(notify).toHaveBeenCalledWith("Select a running worker before steering", "warning");
  });
```

Update every existing `new SymphonyConsoleComponent({ ... })` construction in `console.test.ts` to include:

```ts
      respondToEscalation: async () => undefined,
```

- [ ] **Step 2: Run the failing console tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/console.test.ts
```

Expected: FAIL because the console only renders running workers and uses worker-only selection.

- [ ] **Step 3: Change console imports and options**

In `apps/symphony/pi-extension/src/console.ts`, replace the console-model import with:

```ts
import { buildEscalationRows, buildIssueRows, buildWorkerRows, formatEventRows, type EscalationRow, type IssueRow, type WorkerRow } from "./console-model.ts";
```

Update `ConsoleShortcutAction`:

```ts
export type ConsoleShortcutAction = "selectPrevious" | "selectNext" | "refresh" | "steer" | "respondEscalation" | "toggleDetails" | "close";
```

Add `respondToEscalation` to `ConsoleOptions`:

```ts
  respondToEscalation: (requestId: string, response: unknown) => Promise<void>;
```

In `handleActiveConsoleShortcut()`, add:

```ts
    case "respondEscalation":
      await activeConsole.respondToEscalationNow();
      return;
```

- [ ] **Step 4: Track selection across issue rows**

In `SymphonyConsoleComponent`, replace `private selectedWorkerIndex = 0;` with:

```ts
  private selectedIndex = 0;
```

In `render()`, replace the worker-only row setup with:

```ts
    const symphonyState = this.options.getState();
    const issueRows = buildIssueRows(symphonyState);
    const escalationRows = buildEscalationRows(symphonyState);
    this.clampSelection(issueRows.length + escalationRows.length);
    const selectedIssue = issueRows[this.selectedIndex];
```

Keep `workers` for count fallback by adding:

```ts
    const workers = buildWorkerRows(symphonyState);
```

Replace the running table and selected detail sections in `lines` with:

```ts
      ...boxLines("Running Workers", renderIssueTable(issueRows.filter((row) => row.kind === "running"), issueRows, this.selectedIndex, theme), consoleWidth, theme),
      ...boxLines("Retry Queue", renderIssueTable(issueRows.filter((row) => row.kind === "retry"), issueRows, this.selectedIndex, theme), consoleWidth, theme),
      ...boxLines("Blocked Issues", renderIssueTable(issueRows.filter((row) => row.kind === "blocked"), issueRows, this.selectedIndex, theme), consoleWidth, theme),
      ...boxLines("Completed Issues", renderIssueTable(issueRows.filter((row) => row.kind === "completed"), issueRows, this.selectedIndex, theme), consoleWidth, theme),
      ...boxLines("Selected Issue", renderSelectedIssueDetails(selectedIssue, state.console.showDetails, theme), consoleWidth, theme),
```

- [ ] **Step 5: Update movement and steering**

Replace `moveSelection()` and `clampSelection()` with:

```ts
  private moveSelection(delta: number): void {
    const state = this.options.getState();
    const selectableCount = buildIssueRows(state).length + buildEscalationRows(state).length;
    if (selectableCount === 0) return;
    this.selectedIndex = Math.max(0, Math.min(selectableCount - 1, this.selectedIndex + delta));
    this.options.requestRender();
  }

  private clampSelection(selectableCount: number): void {
    if (selectableCount === 0) {
      this.selectedIndex = 0;
      return;
    }
    this.selectedIndex = Math.max(0, Math.min(selectableCount - 1, this.selectedIndex));
  }
```

Replace `steerSelectedWorker()` with:

```ts
  private async steerSelectedWorker(): Promise<void> {
    const issueRows = buildIssueRows(this.options.getState());
    this.clampSelection(issueRows.length + buildEscalationRows(this.options.getState()).length);
    const issue = issueRows[this.selectedIndex];
    if (!issue || issue.kind !== "running") {
      this.options.notify("Select a running worker before steering", "warning");
      return;
    }

    try {
      const instruction = (await this.options.prompt("Steer Symphony worker", `Instruction for ${issue.issueIdentifier}`))?.trim();
      if (!instruction) return;

      await this.options.steer(issue.issueIdentifier, instruction);
      this.options.notify(`Steer delivered to ${issue.issueIdentifier}`, "info");
    } catch (error) {
      this.options.notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      this.options.requestRender();
    }
  }
```

- [ ] **Step 6: Add issue render helpers**

Replace `renderWorkerTable()` and `renderSelectedWorkerDetails()` with:

```ts
function renderIssueTable(rows: IssueRow[], allIssueRows: IssueRow[], selectedIndex: number, theme?: ConsoleTheme): string[] {
  const lines = [color(theme, "dim", "sel issue    kind       status              attempt host      detail")];
  if (rows.length === 0) return [...lines, color(theme, "dim", "-   none")];

  for (const row of rows) {
    const globalIndex = allIssueRows.indexOf(row);
    const selected = globalIndex === selectedIndex ? ">" : " ";
    const detail = row.kind === "blocked" ? row.blockers : row.kind === "completed" ? row.completedAt : row.errorPreview;
    const line = [
      selected,
      pad(row.issueIdentifier, 8),
      pad(row.kind, 10),
      pad(row.status, 19),
      pad(row.attempt, 7),
      pad(row.workerHost, 9),
      detail,
    ].join(" ");
    lines.push(globalIndex === selectedIndex ? selectedLine(theme, line) : line);
  }
  return lines;
}

function renderSelectedIssueDetails(issue: IssueRow | undefined, showDetails: boolean, theme?: ConsoleTheme): string[] {
  if (!showDetails) return [];
  if (!issue) return [color(theme, "dim", "none")];
  const lines = [
    `issue: ${color(theme, "accent", issue.issueIdentifier)} ${issue.title}`,
    `kind: ${issue.kind}`,
    `status: ${color(theme, issue.kind === "blocked" ? "error" : issue.kind === "retry" ? "warning" : "success", issue.status)}`,
  ];
  if (issue.kind === "running") {
    lines.push(
      `tracker state: ${color(theme, "success", issue.trackerState)}`,
      `attempt: ${issue.attempt}`,
      `turns: ${issue.turnCount} / ${issue.maxTurns}`,
      `last activity: ${color(theme, "dim", issue.lastActivity)}`,
      `worker host: ${issue.workerHost}`,
      `workspace: ${color(theme, "dim", issue.workspacePath)}`,
      `error: ${issue.errorPreview === "-" ? color(theme, "dim", issue.errorPreview) : color(theme, "error", issue.errorPreview)}`,
    );
  }
  if (issue.kind === "retry") {
    lines.push(
      `attempt: ${issue.attempt}`,
      `worker host: ${issue.workerHost}`,
      `workspace: ${color(theme, "dim", issue.workspacePath)}`,
      `error: ${issue.errorPreview === "-" ? color(theme, "dim", issue.errorPreview) : color(theme, "error", issue.errorPreview)}`,
    );
  }
  if (issue.kind === "blocked") {
    lines.push(`blockers: ${color(theme, "warning", issue.blockers)}`);
  }
  if (issue.kind === "completed") {
    lines.push(`completed at: ${color(theme, "dim", issue.completedAt)}`);
  }
  return lines;
}
```

- [ ] **Step 7: Run console tests and commit**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/console.test.ts
```

Expected: PASS after updating earlier worker-specific assertions to expect `Selected Issue` instead of `Selected Worker` where needed.

Commit:

```bash
git add apps/symphony/pi-extension/src/console.ts apps/symphony/pi-extension/src/console.test.ts
git commit -m "feat(pi-symphony): render wave 3 issue buckets"
```

---

### Task 4: Record escalation lifecycle events and respond through runtime

**Files:**
- Modify: `apps/symphony/pi-extension/src/runtime.ts`
- Test: `apps/symphony/pi-extension/src/runtime.test.ts`

- [ ] **Step 1: Write failing runtime tests**

Append these tests inside `describe("SymphonyRuntime", () => { ... })` in `apps/symphony/pi-extension/src/runtime.test.ts`:

```ts
  it("responds to an escalation and refreshes state", async () => {
    const runtime = new SymphonyRuntime();
    const response = {
      running: {},
      retry_queue: [],
      blocked: [],
      completed: [],
      pending_escalations: [],
      polling: { checking: false, next_poll_in_ms: 1000, poll_interval_ms: 30000 },
    };
    runtime.client = {
      respondEscalation: vi.fn(async () => ({ ok: true })),
      getState: vi.fn(async () => response),
      toHealthSummary: vi.fn(() => lastKnownState("http://127.0.0.1:8080")),
    } as unknown as SymphonyRuntime["client"];

    await expect(runtime.respondToEscalation("esc-1", { approved: true })).resolves.toEqual({ ok: true });

    expect(runtime.client?.respondEscalation).toHaveBeenCalledWith("esc-1", { approved: true }, "pi-dashboard", undefined);
    expect(runtime.client?.getState).toHaveBeenCalledOnce();
    expect(runtime.lastState).toBe(response);
  });

  it("returns escalation response result even if post-response refresh fails", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const runtime = new SymphonyRuntime();
    runtime.client = {
      respondEscalation: vi.fn(async () => ({ ok: true })),
      getState: vi.fn(async () => {
        throw new Error("temporary state fetch failure");
      }),
      toHealthSummary: vi.fn(() => lastKnownState("http://127.0.0.1:8080")),
    } as unknown as SymphonyRuntime["client"];

    await expect(runtime.respondToEscalation("esc-1", "approved")).resolves.toEqual({ ok: true });

    expect(warn).toHaveBeenCalledWith("Symphony state refresh failed after escalation response", expect.any(Error));
  });

  it("retains recent escalation lifecycle events", () => {
    const runtime = new SymphonyRuntime();

    runtime.recordEvent({ version: "v1", sequence: 1, timestamp: "2026-05-14T12:00:00Z", kind: "escalation_created", severity: "info", issue: "SIM-123", event: "escalation_created", payload: {} });
    runtime.recordEvent({ version: "v1", sequence: 2, timestamp: "2026-05-14T12:01:00Z", kind: "escalation_responded", severity: "info", issue: "SIM-123", event: "escalation_responded", payload: {} });
    runtime.recordEvent({ version: "v1", sequence: 3, timestamp: "2026-05-14T12:02:00Z", kind: "heartbeat", severity: "info", event: "heartbeat", payload: {} });

    expect(runtime.recentEvents.map((event) => event.kind)).toEqual(["escalation_created", "escalation_responded"]);
  });
```

- [ ] **Step 2: Run failing runtime tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/runtime.test.ts
```

Expected: FAIL because `respondToEscalation()` does not exist and escalation events are ignored.

- [ ] **Step 3: Add runtime response method**

In `apps/symphony/pi-extension/src/runtime.ts`, update the import from `http-client.ts`:

```ts
import { SymphonyHttpClient, type EscalationRespondResponse, type SteerResponse, type SymphonyEventEnvelope, type SymphonyStateResponse } from "./http-client.ts";
```

Add this method after `steerWorker()`:

```ts
  async respondToEscalation(requestId: string, response: unknown, signal?: AbortSignal): Promise<EscalationRespondResponse> {
    if (!this.client) throw new SymphonyExtensionError("no_attachment", "No Symphony server is attached");
    const result = await this.client.respondEscalation(requestId, response, "pi-dashboard", signal);
    try {
      await this.refreshState(signal);
    } catch (error) {
      console.warn("Symphony state refresh failed after escalation response", error);
    }
    return result;
  }
```

Replace `recordEvent()` with:

```ts
  recordEvent(event: SymphonyEventEnvelope): void {
    if (event.kind !== "worker" && event.kind !== "runtime" && !event.kind.startsWith("escalation_")) return;
    this.recentEvents = [...this.recentEvents, event].slice(-20);
  }
```

- [ ] **Step 4: Run runtime tests and commit**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/runtime.test.ts
```

Expected: PASS.

Commit:

```bash
git add apps/symphony/pi-extension/src/runtime.ts apps/symphony/pi-extension/src/runtime.test.ts
git commit -m "feat(pi-symphony): respond to escalations through runtime"
```

---

### Task 5: Render pending escalations and dashboard response flow

**Files:**
- Modify: `apps/symphony/pi-extension/src/console.ts`
- Test: `apps/symphony/pi-extension/src/console.test.ts`

- [ ] **Step 1: Write failing escalation UI tests**

In `workerStateFixture()` in `apps/symphony/pi-extension/src/console.test.ts`, add:

```ts
    pending_escalations: [{ request_id: "esc-1", issue_id: "issue-123", issue_identifier: "SIM-123", method: "approval", preview: "Approve cargo test?", created_at: "2026-05-14T12:06:00Z", timeout_ms: 600000 }],
```

Add these tests after the Wave 3 issue rendering test:

```ts
  it("renders pending escalations and selected escalation details", () => {
    const state = createDefaultState();
    state.console.showDetails = true;
    const consoleComponent = new SymphonyConsoleComponent({
      state,
      getState: () => workerStateFixture(),
      getEvents: () => [],
      refresh: async () => undefined,
      steer: async () => undefined,
      respondToEscalation: async () => undefined,
      prompt: async () => undefined,
      close: () => undefined,
      requestRender: () => undefined,
      notify: () => undefined,
    });

    for (let index = 0; index < 5; index += 1) consoleComponent.handleInput("\u001b[B");

    const output = consoleComponent.render(180).join("\n");
    expect(output).toContain("Pending Escalations");
    expect(output).toContain("> esc-1");
    expect(output).toContain("SIM-123");
    expect(output).toContain("approval");
    expect(output).toContain("Approve cargo test?");
    expect(output).toContain("Selected Escalation");
    expect(output).toContain("request: esc-1");
    expect(output).toContain("timeout: 10m 0s");
  });

  it("responds to the selected escalation with parsed JSON", async () => {
    const respondToEscalation = vi.fn(async () => undefined);
    const notify = vi.fn();
    const consoleComponent = new SymphonyConsoleComponent({
      state: createDefaultState(),
      getState: () => workerStateFixture(),
      getEvents: () => [],
      refresh: async () => undefined,
      steer: async () => undefined,
      respondToEscalation,
      prompt: async () => '{"approved":true}',
      close: () => undefined,
      requestRender: () => undefined,
      notify,
    });

    for (let index = 0; index < 5; index += 1) consoleComponent.handleInput("\u001b[B");
    consoleComponent.handleInput("e");

    await expect.poll(() => notify.mock.calls.length, { interval: 10, timeout: 1000 }).toBe(1);
    expect(respondToEscalation).toHaveBeenCalledWith("esc-1", { approved: true });
    expect(notify).toHaveBeenCalledWith("Escalation response sent for esc-1", "info");
  });

  it("responds to the selected escalation with plain text when input is not JSON", async () => {
    const respondToEscalation = vi.fn(async () => undefined);
    const consoleComponent = new SymphonyConsoleComponent({
      state: createDefaultState(),
      getState: () => workerStateFixture(),
      getEvents: () => [],
      refresh: async () => undefined,
      steer: async () => undefined,
      respondToEscalation,
      prompt: async () => "approved",
      close: () => undefined,
      requestRender: () => undefined,
      notify: () => undefined,
    });

    for (let index = 0; index < 5; index += 1) consoleComponent.handleInput("\u001b[B");
    consoleComponent.handleInput("e");

    await expect.poll(() => respondToEscalation.mock.calls.length, { interval: 10, timeout: 1000 }).toBe(1);
    expect(respondToEscalation).toHaveBeenCalledWith("esc-1", "approved");
  });

  it("notifies when responding without a selected escalation", async () => {
    const notify = vi.fn();
    const consoleComponent = new SymphonyConsoleComponent({
      state: createDefaultState(),
      getState: () => workerStateFixture(),
      getEvents: () => [],
      refresh: async () => undefined,
      steer: async () => undefined,
      respondToEscalation: async () => undefined,
      prompt: async () => "approved",
      close: () => undefined,
      requestRender: () => undefined,
      notify,
    });

    consoleComponent.handleInput("e");
    expect(notify).toHaveBeenCalledWith("Select an escalation before responding", "warning");
  });
```

- [ ] **Step 2: Run failing escalation UI tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/console.test.ts
```

Expected: FAIL because escalation rendering and response input do not exist yet.

- [ ] **Step 3: Add escalation keyboard action**

In `handleInput(data: string)`, add this block after the `s`/`S` steer block:

```ts
    if (data === "e" || data === "E") {
      void this.respondToEscalationNow();
      return;
    }
```

Add this public method after `steerNow()`:

```ts
  async respondToEscalationNow(): Promise<void> {
    await this.respondToSelectedEscalation();
  }
```

- [ ] **Step 4: Render pending escalations and selected escalation details**

In the `lines` array in `render()`, insert this section after `Selected Issue`:

```ts
      ...boxLines("Pending Escalations", renderEscalationTable(escalationRows, this.selectedIndex - issueRows.length, theme), consoleWidth, theme),
      ...boxLines("Selected Escalation", renderSelectedEscalationDetails(escalationRows[this.selectedIndex - issueRows.length], state.console.showDetails, theme), consoleWidth, theme),
```

Add these helper functions below `renderSelectedIssueDetails()`:

```ts
function renderEscalationTable(rows: EscalationRow[], selectedEscalationIndex: number, theme?: ConsoleTheme): string[] {
  const lines = [color(theme, "dim", "sel request  issue    method     timeout   preview")];
  if (rows.length === 0) return [...lines, color(theme, "dim", "-   no pending escalations")];

  for (const [index, row] of rows.entries()) {
    const selected = index === selectedEscalationIndex ? ">" : " ";
    const line = [
      selected,
      pad(row.requestId, 8),
      pad(row.issueIdentifier, 8),
      pad(row.method, 10),
      pad(row.timeout, 9),
      row.preview,
    ].join(" ");
    lines.push(index === selectedEscalationIndex ? selectedLine(theme, line) : line);
  }
  return lines;
}

function renderSelectedEscalationDetails(escalation: EscalationRow | undefined, showDetails: boolean, theme?: ConsoleTheme): string[] {
  if (!showDetails) return [];
  if (!escalation) return [color(theme, "dim", "none")];
  return [
    `request: ${color(theme, "accent", escalation.requestId)}`,
    `issue: ${color(theme, "accent", escalation.issueIdentifier)}`,
    `method: ${escalation.method}`,
    `created: ${color(theme, "dim", escalation.createdAt)}`,
    `timeout: ${color(theme, "warning", escalation.timeout)}`,
    `preview: ${escalation.preview}`,
  ];
}
```

- [ ] **Step 5: Add escalation response flow**

Add these private methods inside `SymphonyConsoleComponent` after `steerSelectedWorker()`:

```ts
  private async respondToSelectedEscalation(): Promise<void> {
    const state = this.options.getState();
    const issueRows = buildIssueRows(state);
    const escalationRows = buildEscalationRows(state);
    this.clampSelection(issueRows.length + escalationRows.length);
    const escalation = escalationRows[this.selectedIndex - issueRows.length];
    if (!escalation) {
      this.options.notify("Select an escalation before responding", "warning");
      return;
    }

    try {
      const rawResponse = (await this.options.prompt("Respond to Symphony escalation", `Response for ${escalation.requestId}`))?.trim();
      if (!rawResponse) return;
      await this.options.respondToEscalation(escalation.requestId, parseEscalationResponseInput(rawResponse));
      this.options.notify(`Escalation response sent for ${escalation.requestId}`, "info");
    } catch (error) {
      this.options.notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      this.options.requestRender();
    }
  }
```

Add this module-level helper above `renderRecentEvents()`:

```ts
function parseEscalationResponseInput(value: string): unknown {
  try {
    return JSON.parse(value) as unknown;
  } catch {
    return value;
  }
}
```

- [ ] **Step 6: Wire runtime response into openConsole**

In `openConsole()`, inside the `new SymphonyConsoleComponent({ ... })` options, add:

```ts
      respondToEscalation: async (requestId, response) => {
        await runtime.respondToEscalation(requestId, response);
      },
```

- [ ] **Step 7: Update action legend**

Replace the `keyboard` string in `renderActionLegend()` with:

```ts
  const keyboard = "Keyboard: ctrl+shift+↑/↓ select  •  ctrl+shift+r refresh  •  ctrl+shift+t steer  •  ctrl+shift+e escalation  •  ctrl+shift+i details  •  ctrl+shift+q close";
```

Replace the narrow rendering branch with:

```ts
  return [
    "Keyboard: ctrl+shift+↑/↓ select  •  ctrl+shift+r refresh  •  ctrl+shift+t steer",
    "          ctrl+shift+e escalation  •  ctrl+shift+i details  •  ctrl+shift+q close",
    commands,
  ];
```

- [ ] **Step 8: Run console tests and commit**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/console.test.ts
```

Expected: PASS after updating shortcut legend assertions to use `ctrl+shift+t steer` and `ctrl+shift+e escalation`.

Commit:

```bash
git add apps/symphony/pi-extension/src/console.ts apps/symphony/pi-extension/src/console.test.ts
git commit -m "feat(pi-symphony): support dashboard escalation responses"
```

---

### Task 6: Update command shortcuts and help text

**Files:**
- Modify: `apps/symphony/pi-extension/src/commands.ts`
- Test: `apps/symphony/pi-extension/src/commands.test.ts`

- [ ] **Step 1: Write failing shortcut tests**

In `apps/symphony/pi-extension/src/commands.test.ts`, replace the shortcut assertion in `registers console keyboard shortcuts` with:

```ts
    expect([...shortcuts.keys()]).toEqual(expect.arrayContaining([
      "ctrl+shift+up",
      "ctrl+shift+down",
      "ctrl+shift+r",
      "ctrl+shift+t",
      "ctrl+shift+e",
      "ctrl+shift+i",
      "ctrl+shift+q",
    ]));
    expect(shortcuts.get("ctrl+shift+down")?.description).toContain("Select next Symphony console item");
    expect(shortcuts.get("ctrl+shift+t")?.description).toContain("Steer the selected Symphony console worker");
    expect(shortcuts.get("ctrl+shift+e")?.description).toContain("Respond to the selected Symphony console escalation");
```

- [ ] **Step 2: Run failing command tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/commands.test.ts
```

Expected: FAIL because `ctrl+shift+e` is not registered for escalation responses yet.

- [ ] **Step 3: Update shortcut registrations**

In `registerConsoleShortcuts()` in `apps/symphony/pi-extension/src/commands.ts`, replace the shortcuts array with:

```ts
  const shortcuts = [
    { key: "ctrl+shift+up", action: "selectPrevious", description: "Select previous Symphony console item" },
    { key: "ctrl+shift+down", action: "selectNext", description: "Select next Symphony console item" },
    { key: "ctrl+shift+r", action: "refresh", description: "Refresh the Symphony console" },
    { key: "ctrl+shift+t", action: "steer", description: "Steer the selected Symphony console worker" },
    { key: "ctrl+shift+e", action: "respondEscalation", description: "Respond to the selected Symphony console escalation" },
    { key: "ctrl+shift+i", action: "toggleDetails", description: "Toggle Symphony console item details" },
    { key: "ctrl+shift+q", action: "close", description: "Close the Symphony console widget" },
  ] as const satisfies ReadonlyArray<{ key: KeyId; action: ConsoleShortcutAction; description: string }>;
```

In `helpText()`, add a short controls block after the command list by replacing the return array tail with:

```ts
    "/symphony:steer <ISSUE> <instruction>",
    "/symphony:stop",
    "",
    "Console keys:",
    "↑/↓ select items",
    "r refresh, s steer selected running worker, e respond to selected escalation",
    "d details, q/Escape close",
  ].join("\n");
```

- [ ] **Step 4: Run command tests and commit**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/commands.test.ts
```

Expected: PASS.

Commit:

```bash
git add apps/symphony/pi-extension/src/commands.ts apps/symphony/pi-extension/src/commands.test.ts
git commit -m "feat(pi-symphony): add escalation console shortcuts"
```

---

### Task 7: Document Wave 3 and run full verification

**Files:**
- Modify: `apps/symphony/pi-extension/README.md`

- [ ] **Step 1: Update README commands and keys**

In `apps/symphony/pi-extension/README.md`, replace `## Commands through Slice 2` with:

```md
## Commands through Wave 3
```

Replace `## Console keys through Slice 2` and its bullet list with:

```md
## Console keys through Wave 3

- `↑` / `↓` selects running workers, retry entries, blocked issues, completed issues, and pending escalations.
- `r` requests an immediate Symphony refresh and reloads state.
- `s` prompts for a steer instruction when the selected item is a running worker.
- `e` prompts for a response when the selected item is a pending escalation. Valid JSON is sent as JSON; other input is sent as a string response.
- `d` toggles selected-item details.
- `q` or Escape closes the console and leaves Symphony running.
```

Append this section after `## Slice 1 verification`:

```md
## Wave 3 manual verification

1. Start Pi with the local extension:

   ```sh
   pi -e ./apps/symphony/pi-extension
   ```

   Expected: Pi starts with Symphony commands available.

2. Start or attach to a Symphony server:

   ```text
   /symphony:start .symphony/WORKFLOW.md
   ```

   or:

   ```text
   /symphony:attach http://127.0.0.1:<port>
   ```

   Expected: the Symphony console opens and the status block shows the attached base URL.

3. Create or use a Symphony state with at least one retry entry, blocked issue, completed issue, and pending escalation.

   Expected: the console shows `Retry Queue`, `Blocked Issues`, `Completed Issues`, and `Pending Escalations` sections.

4. Use `↑` / `↓` to select each Wave 3 item type.

   Expected: the detail panel shows running, retry, blocked, completed, and escalation-specific fields.

5. Select a pending escalation, press `e`, and enter `{"approved":true}`.

   Expected: Pi reports `Escalation response sent for <request_id>` and the console refreshes.

6. Watch recent events after escalation creation and response.

   Expected: escalation lifecycle events appear in the `Events` section.
```

- [ ] **Step 2: Run focused test suite**

Run:

```bash
pnpm --dir apps/symphony/pi-extension test
```

Expected: PASS for the pi-extension Vitest suite.

- [ ] **Step 3: Run typecheck**

Run:

```bash
pnpm --dir apps/symphony/pi-extension typecheck
```

Expected: PASS with no TypeScript errors.

- [ ] **Step 4: Run affected validation from repo root**

Run:

```bash
pnpm run validate:affected
```

Expected: PASS for affected Turborepo tasks.

- [ ] **Step 5: Commit docs and verification adjustments**

Commit:

```bash
git add apps/symphony/pi-extension/README.md
git commit -m "docs(pi-symphony): document wave 3 console controls"
```

---

## Self-Review

**Spec coverage:**
- Render retry queue, blocked issues, and completed issues: Tasks 2 and 3.
- Add an issue detail panel for running, retry, blocked, and completed states: Task 3.
- Render pending escalations: Tasks 2 and 5.
- Support responding to escalations from the dashboard: Tasks 1, 4, and 5.
- Reflect escalation lifecycle events: Tasks 2 and 4.

**Red-flag scan:** No banned placeholder markers, vague validation instructions, or cross-task copy references remain.

**Type consistency:** The plan consistently uses `PendingEscalationResponse`, `EscalationRow`, `IssueRow`, `respondEscalation()`, `respondToEscalation()`, and `respondToEscalationNow()` across client, runtime, model, and console tasks.
