---
type: Plan
title: Wave 4 Symphony Shared Context and Diagnostics
description: "Implementation plan for Wave 4 dashboard parity: shared context and diagnostics."
tags: [symphony, pi, dashboard]
timestamp: 2026-07-15T20:00:00Z
---

# Wave 4 Symphony Shared Context and Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Wave 4 dashboard parity for Symphony shared context operations and diagnostics in the Pi Symphony extension.

**Architecture:** Keep HTTP contracts and validators in `http-client.ts`, model transformation in `console-model.ts`, runtime state in `runtime.ts`, and rendering and key handling in `console.ts`. The console stays a single Pi widget with one unified selection index across issues, escalations, and shared context rows.

**Tech Stack:** TypeScript, Vitest, Node HTTP mock server, Pi extension APIs, `ws` for Symphony event streams.

Related: [Pi Symphony Extension Design](/superpowers/specs/_archive/2026-05-14-pi-symphony-extension-design.md). Roadmap: [specs/index.md](/specs/index.md).

---

## Scope Check

Wave 4 covers two related slices in the same dashboard surface:

- Slice 5: shared context display, create, delete, and scope filtering.
- Slice 6: diagnostics parity for polling, rate limits, token totals, event stream counters, event filters, help text, key hints, empty states, and layout polish.

These share the same HTTP client, runtime attachment, event stream, and console widget. Keep them in one plan so each task produces a working dashboard increment.

## File Structure

- Modify: `apps/symphony/pi-extension/src/http-client.ts`
  - Owns typed shared context, diagnostics state contracts, context endpoints, and response validation.
- Modify: `apps/symphony/pi-extension/src/http-client.test.ts`
  - Verifies shared context API calls, diagnostics state validation, and API error normalization.
- Modify: `apps/symphony/pi-extension/src/event-stream.ts`
  - Adds event stream query filters for issue, event type, and severity.
- Modify: `apps/symphony/pi-extension/src/event-stream.test.ts`
  - Verifies websocket URLs include encoded filters.
- Modify: `apps/symphony/pi-extension/src/state.ts`
  - Persists console scope filter and event filters.
- Modify: `apps/symphony/pi-extension/src/state.test.ts`
  - Verifies filter persistence and legacy restore behavior.
- Modify: `apps/symphony/pi-extension/src/runtime.ts`
  - Stores latest shared context payload, exposes create and delete operations, tracks event stream counters, and records shared context and supervisor events.
- Modify: `apps/symphony/pi-extension/src/runtime.test.ts`
  - Verifies runtime context operations and event counter behavior.
- Modify: `apps/symphony/pi-extension/src/console-model.ts`
  - Builds shared context rows, summary rows, diagnostics rows, rate limit summaries, and filtered event rows.
- Modify: `apps/symphony/pi-extension/src/console-model.test.ts`
  - Verifies formatting for context rows, diagnostics, rate limits, and filters.
- Modify: `apps/symphony/pi-extension/src/console.ts`
  - Renders shared context and diagnostics sections; adds create, delete, scope filter, and event filter keys.
- Modify: `apps/symphony/pi-extension/src/console.test.ts`
  - Verifies rendering, selection, create/delete prompts, scope filters, event filters, and empty states.
- Modify: `apps/symphony/pi-extension/src/commands.ts`
  - Registers new global shortcuts and updates help text.
- Modify: `apps/symphony/pi-extension/src/commands.test.ts`
  - Verifies shortcut registration and help text.
- Create: `apps/symphony/pi-extension/scripts/wave4-mock-server.mjs`
  - Serves Wave 4 state, shared context endpoints, escalation endpoints, refresh, steer, and debug endpoints.
- Create: `apps/symphony/pi-extension/src/wave4-mock-server.test.ts`
  - Verifies mock server context CRUD and diagnostics state.
- Modify: `apps/symphony/pi-extension/package.json`
  - Adds `mock:wave4` script.
- Modify: `apps/symphony/pi-extension/README.md`
  - Documents Wave 4 console keys and manual verification.
- Modify: `docs/superpowers/specs/_archive/2026-05-14-pi-symphony-extension-design.md`
  - Marks Wave 4 complete after implementation and verification.

## Assumptions

- Symphony already exposes `GET /api/v1/context`, `POST /api/v1/context`, `DELETE /api/v1/context`, `DELETE /api/v1/context/:entry_id`, and filtered websocket `GET /api/v1/events?issue=&type=&severity=`.
- Scope strings accepted by Symphony are `project`, `milestone:<id>`, and `label:<name>`.
- The Pi `ui.input(title, label)` API returns one string, so create-context and event-filter prompts use compact, documented input formats.

---

### Task 1: Add shared context and diagnostics HTTP contracts

**Files:**
- Modify: `apps/symphony/pi-extension/src/http-client.ts`
- Modify: `apps/symphony/pi-extension/src/http-client.test.ts`

- [ ] **Step 1: Write failing HTTP client tests**

Add these tests inside `describe("SymphonyHttpClient", () => { ... })` in `apps/symphony/pi-extension/src/http-client.test.ts` after the escalation tests.

```ts
  it("fetches shared context with an encoded scope filter", async () => {
    const entries = [
      {
        id: "ctx-1",
        author_issue: "SIM-123",
        scope: { type: "milestone", value: "M001" },
        content: "Decision: use the existing auth module",
        created_at: "2026-05-17T12:00:00Z",
        ttl_ms: 3600000,
      },
    ];
    const summary = {
      total_entries: 1,
      entries_by_scope: { "milestone:M001": 1 },
      oldest_entry_at: "2026-05-17T12:00:00Z",
      newest_entry_at: "2026-05-17T12:00:00Z",
    };
    const baseUrl = await serve((req) => {
      expect(req.method).toBe("GET");
      expect(req.url).toBe("/api/v1/context?scope=milestone%3AM001");
      return { status: 200, body: { entries, summary } };
    });

    const client = new SymphonyHttpClient(baseUrl);

    await expect(client.getContext("milestone:M001")).resolves.toEqual({ entries, summary });
  });

  it("creates shared context entries", async () => {
    const baseUrl = await serve((req, body) => {
      expect(req.method).toBe("POST");
      expect(req.url).toBe("/api/v1/context");
      expect(JSON.parse(body)).toEqual({
        author_issue: "SIM-123",
        scope: "project",
        content: "Decision: keep context in the extension package",
        ttl_ms: 60000,
      });
      return { status: 201, body: { id: "ctx-2", created_at: "2026-05-17T12:01:00Z" } };
    });

    const client = new SymphonyHttpClient(baseUrl);

    await expect(client.createContext({
      authorIssue: "SIM-123",
      scope: "project",
      content: "Decision: keep context in the extension package",
      ttlMs: 60000,
    })).resolves.toEqual({ id: "ctx-2", created_at: "2026-05-17T12:01:00Z" });
  });

  it("deletes one shared context entry by id", async () => {
    const baseUrl = await serve((req) => {
      expect(req.method).toBe("DELETE");
      expect(req.url).toBe("/api/v1/context/ctx-1");
      return { status: 200, body: { deleted: 1 } };
    });

    const client = new SymphonyHttpClient(baseUrl);

    await expect(client.deleteContextEntry("ctx-1")).resolves.toEqual({ deleted: 1 });
  });

  it("clears shared context by scope", async () => {
    const baseUrl = await serve((req) => {
      expect(req.method).toBe("DELETE");
      expect(req.url).toBe("/api/v1/context?scope=label%3Abackend");
      return { status: 200, body: { deleted: 2 } };
    });

    const client = new SymphonyHttpClient(baseUrl);

    await expect(client.deleteContext("label:backend")).resolves.toEqual({ deleted: 2 });
  });

  it("fetches typed Wave 4 diagnostics from state", async () => {
    const baseUrl = await serve(() => ({
      status: 200,
      body: validState({
        shared_context: {
          total_entries: 2,
          entries_by_scope: { project: 1, "milestone:M001": 1 },
          oldest_entry_at: "2026-05-17T12:00:00Z",
          newest_entry_at: "2026-05-17T12:05:00Z",
        },
        supervisor: {
          active: true,
          steers_issued: 3,
          conflicts_detected: 1,
          patterns_detected: 2,
          escalations_created: 4,
        },
        codex_totals: {
          input_tokens: 1000,
          output_tokens: 500,
          total_tokens: 1500,
          event_count: 12,
          seconds_running: 90,
        },
        codex_rate_limits: {
          requests: { remaining: 80, limit: 100, reset_seconds: 120 },
        },
        polling: {
          checking: true,
          next_poll_in_ms: 2500,
          poll_interval_ms: 30000,
          poll_count: 7,
          last_poll_at: "2026-05-17T12:05:00Z",
        },
      }),
    }));

    const client = new SymphonyHttpClient(baseUrl);
    const state = await client.getState();

    expect(state.shared_context).toMatchObject({ total_entries: 2 });
    expect(state.supervisor).toMatchObject({ active: true, steers_issued: 3 });
    expect(state.codex_totals).toMatchObject({ total_tokens: 1500, event_count: 12 });
    expect(state.codex_rate_limits).toMatchObject({ requests: { remaining: 80, limit: 100 } });
    expect(state.polling?.poll_count).toBe(7);
  });

  it.each([
    ["shared_context.total_entries", validState({ shared_context: { entries_by_scope: {}, oldest_entry_at: null, newest_entry_at: null } })],
    ["shared_context.entries_by_scope", validState({ shared_context: { total_entries: 1, entries_by_scope: [], oldest_entry_at: null, newest_entry_at: null } })],
    ["supervisor.steers_issued", validState({ supervisor: { active: true, conflicts_detected: 0, patterns_detected: 0, escalations_created: 0 } })],
    ["codex_totals.total_tokens", validState({ codex_totals: { input_tokens: 1, output_tokens: 2, event_count: 3, seconds_running: 4 } })],
  ])("rejects malformed Wave 4 state field: %s", async (field, body) => {
    const baseUrl = await serve(() => ({ status: 200, body }));
    const client = new SymphonyHttpClient(baseUrl);

    await expect(client.getState()).rejects.toMatchObject({
      kind: "non_symphony_response",
      details: expect.objectContaining({ field }),
    } satisfies Partial<SymphonyExtensionError>);
  });
```

Update `validState()` in the same file so existing tests continue to use a full Wave 4 state payload.

```ts
    shared_context: { total_entries: 0, entries_by_scope: {}, oldest_entry_at: null, newest_entry_at: null },
    supervisor: { active: true, steers_issued: 0, conflicts_detected: 0, patterns_detected: 0, escalations_created: 0 },
    codex_totals: { input_tokens: 0, output_tokens: 0, total_tokens: 0, event_count: 0, seconds_running: 0 },
    codex_rate_limits: null,
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/http-client.test.ts
```

Expected: FAIL with errors that `getContext`, `createContext`, `deleteContextEntry`, and `deleteContext` do not exist, or Wave 4 fields are not typed.

- [ ] **Step 3: Add HTTP contracts and methods**

In `apps/symphony/pi-extension/src/http-client.ts`, add these interfaces after `EscalationRespondResponse`.

```ts
export type ContextScopeResponse =
  | { type: "project" }
  | { type: "milestone"; value: string }
  | { type: "label"; value: string };

export interface SharedContextEntryResponse {
  id: string;
  author_issue: string;
  scope: ContextScopeResponse;
  content: string;
  created_at: string;
  ttl_ms: number;
}

export interface SharedContextSummaryResponse {
  total_entries: number;
  entries_by_scope: Record<string, number>;
  oldest_entry_at: string | null;
  newest_entry_at: string | null;
}

export interface SharedContextListResponse {
  entries: SharedContextEntryResponse[];
  summary: SharedContextSummaryResponse;
}

export interface SharedContextCreateInput {
  authorIssue: string;
  scope: string;
  content: string;
  ttlMs?: number;
}

export interface SharedContextWriteResponse {
  id: string;
  created_at: string;
}

export interface SharedContextDeleteResponse {
  deleted: number;
}

export interface SupervisorSnapshotResponse {
  active?: boolean;
  status?: string;
  steers_issued: number;
  conflicts_detected: number;
  patterns_detected: number;
  escalations_created: number;
}

export interface CodexTotalsResponse {
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  event_count: number;
  seconds_running: number;
}
```

Extend `SymphonyStateResponse` with these fields.

```ts
  shared_context: SharedContextSummaryResponse;
  supervisor: SupervisorSnapshotResponse;
  codex_totals: CodexTotalsResponse;
  codex_rate_limits: unknown | null;
```

Add these methods to `SymphonyHttpClient` after `respondEscalation(...)`.

```ts
  async getContext(scope?: string, signal?: AbortSignal): Promise<SharedContextListResponse> {
    const trimmedScope = scope?.trim();
    const path = trimmedScope ? `/api/v1/context?scope=${encodeURIComponent(trimmedScope)}` : "/api/v1/context";
    const json = await this.requestJson(path, { method: "GET", signal });
    return validateSharedContextListResponse(json, { baseUrl: this.baseUrl, path });
  }

  async createContext(input: SharedContextCreateInput, signal?: AbortSignal): Promise<SharedContextWriteResponse> {
    const path = "/api/v1/context";
    const body: Record<string, unknown> = {
      author_issue: input.authorIssue,
      scope: input.scope,
      content: input.content,
    };
    if (input.ttlMs !== undefined) body.ttl_ms = input.ttlMs;
    const json = await this.requestJson(path, {
      method: "POST",
      signal,
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    return validateSharedContextWriteResponse(json, { baseUrl: this.baseUrl, path });
  }

  async deleteContext(scope?: string, signal?: AbortSignal): Promise<SharedContextDeleteResponse> {
    const trimmedScope = scope?.trim();
    const path = trimmedScope ? `/api/v1/context?scope=${encodeURIComponent(trimmedScope)}` : "/api/v1/context";
    const json = await this.requestJson(path, { method: "DELETE", signal });
    return validateSharedContextDeleteResponse(json, { baseUrl: this.baseUrl, path });
  }

  async deleteContextEntry(entryId: string, signal?: AbortSignal): Promise<SharedContextDeleteResponse> {
    const path = `/api/v1/context/${encodeURIComponent(entryId)}`;
    const json = await this.requestJson(path, { method: "DELETE", signal });
    return validateSharedContextDeleteResponse(json, { baseUrl: this.baseUrl, path, entryId });
  }
```

- [ ] **Step 4: Add validators**

In `validateSymphonyStateResponse(...)`, add `shared_context`, `supervisor`, `codex_totals`, and `codex_rate_limits` to the required field list.

```ts
  const missingFields = [
    "running",
    "retry_queue",
    "blocked",
    "completed",
    "polling",
    "shared_context",
    "supervisor",
    "codex_totals",
    "codex_rate_limits",
  ].filter((field) => !(field in value));
```

Replace the existing loose Wave 4 validators in `validateSymphonyStateResponse(...)` with these calls.

```ts
  validateSharedContextSummary(value.shared_context, details, "shared_context", throwNonSymphonyState);
  validateSupervisorSnapshot(value.supervisor, details, "supervisor", throwNonSymphonyState);
  validateCodexTotals(value.codex_totals, details, "codex_totals", throwNonSymphonyState);
  if (value.codex_rate_limits !== null && value.codex_rate_limits !== undefined && !isRecord(value.codex_rate_limits)) {
    throwNonSymphonyState(details, "state response field had an invalid shape", { field: "codex_rate_limits", expected: "object or null" });
  }
```

Add these functions before `validateRefreshResponse(...)`.

```ts
function validateSharedContextListResponse(value: unknown, details: Record<string, unknown>): SharedContextListResponse {
  if (!isRecord(value)) throwNonSymphonyContext(details, "shared context response was not an object");
  if (!Array.isArray(value.entries)) {
    throwNonSymphonyContext(details, "shared context response field had an invalid shape", { field: "entries", expected: "array" });
  }
  value.entries.forEach((entry, index) => validateSharedContextEntry(entry, details, `entries.${index}`, throwNonSymphonyContext));
  validateSharedContextSummary(value.summary, details, "summary", throwNonSymphonyContext);
  return value as unknown as SharedContextListResponse;
}

function validateSharedContextWriteResponse(value: unknown, details: Record<string, unknown>): SharedContextWriteResponse {
  if (!isRecord(value)) throwNonSymphonyContext(details, "shared context write response was not an object");
  validateRequiredString(value, "id", details, "id", throwNonSymphonyContext);
  validateRequiredString(value, "created_at", details, "created_at", throwNonSymphonyContext);
  return { id: value.id as string, created_at: value.created_at as string };
}

function validateSharedContextDeleteResponse(value: unknown, details: Record<string, unknown>): SharedContextDeleteResponse {
  if (!isRecord(value)) throwNonSymphonyContext(details, "shared context delete response was not an object");
  validateRequiredNumber(value, "deleted", details, "deleted", throwNonSymphonyContext);
  return { deleted: value.deleted as number };
}

function validateSharedContextEntry(value: unknown, details: Record<string, unknown>, detailField: string, thrower: NonSymphonyThrower): void {
  if (!isRecord(value)) thrower(details, "shared context entry had an invalid shape", { field: detailField, expected: "object" });
  validateRequiredString(value, "id", details, `${detailField}.id`, thrower);
  validateRequiredString(value, "author_issue", details, `${detailField}.author_issue`, thrower);
  validateContextScope(value.scope, details, `${detailField}.scope`, thrower);
  validateRequiredString(value, "content", details, `${detailField}.content`, thrower);
  validateRequiredString(value, "created_at", details, `${detailField}.created_at`, thrower);
  validateRequiredNumber(value, "ttl_ms", details, `${detailField}.ttl_ms`, thrower);
}

function validateContextScope(value: unknown, details: Record<string, unknown>, detailField: string, thrower: NonSymphonyThrower): void {
  if (!isRecord(value)) thrower(details, "shared context scope had an invalid shape", { field: detailField, expected: "object" });
  if (value.type !== "project" && value.type !== "milestone" && value.type !== "label") {
    thrower(details, "shared context scope had an invalid shape", { field: `${detailField}.type`, expected: "project | milestone | label" });
  }
  if ((value.type === "milestone" || value.type === "label") && typeof value.value !== "string") {
    thrower(details, "shared context scope had an invalid shape", { field: `${detailField}.value`, expected: "string" });
  }
}

function validateSharedContextSummary(value: unknown, details: Record<string, unknown>, detailField: string, thrower: NonSymphonyThrower): void {
  if (!isRecord(value)) thrower(details, "shared context summary had an invalid shape", { field: detailField, expected: "object" });
  validateRequiredNumber(value, "total_entries", details, `${detailField}.total_entries`, thrower);
  if (!isRecord(value.entries_by_scope) || Object.values(value.entries_by_scope).some((entry) => !isFiniteNumber(entry))) {
    thrower(details, "shared context summary had an invalid shape", { field: `${detailField}.entries_by_scope`, expected: "Record<string, number>" });
  }
  validateOptionalStringOrNull(value, "oldest_entry_at", details, `${detailField}.oldest_entry_at`);
  validateOptionalStringOrNull(value, "newest_entry_at", details, `${detailField}.newest_entry_at`);
}

function validateSupervisorSnapshot(value: unknown, details: Record<string, unknown>, detailField: string, thrower: NonSymphonyThrower): void {
  if (!isRecord(value)) thrower(details, "supervisor snapshot had an invalid shape", { field: detailField, expected: "object" });
  if (value.active !== undefined && typeof value.active !== "boolean") {
    thrower(details, "supervisor snapshot had an invalid shape", { field: `${detailField}.active`, expected: "boolean" });
  }
  validateOptionalStringOrNull(value, "status", details, `${detailField}.status`);
  validateRequiredNumber(value, "steers_issued", details, `${detailField}.steers_issued`, thrower);
  validateRequiredNumber(value, "conflicts_detected", details, `${detailField}.conflicts_detected`, thrower);
  validateRequiredNumber(value, "patterns_detected", details, `${detailField}.patterns_detected`, thrower);
  validateRequiredNumber(value, "escalations_created", details, `${detailField}.escalations_created`, thrower);
}

function validateCodexTotals(value: unknown, details: Record<string, unknown>, detailField: string, thrower: NonSymphonyThrower): void {
  if (!isRecord(value)) thrower(details, "codex totals had an invalid shape", { field: detailField, expected: "object" });
  validateRequiredNumber(value, "input_tokens", details, `${detailField}.input_tokens`, thrower);
  validateRequiredNumber(value, "output_tokens", details, `${detailField}.output_tokens`, thrower);
  validateRequiredNumber(value, "total_tokens", details, `${detailField}.total_tokens`, thrower);
  validateRequiredNumber(value, "event_count", details, `${detailField}.event_count`, thrower);
  validateRequiredNumber(value, "seconds_running", details, `${detailField}.seconds_running`, thrower);
}

function throwNonSymphonyContext(details: Record<string, unknown>, reason: string, extraDetails: Record<string, unknown> = {}): never {
  throw new SymphonyExtensionError("non_symphony_response", "Response did not look like Symphony shared context response", {
    ...details,
    reason,
    ...extraDetails,
  });
}
```

- [ ] **Step 5: Run HTTP client tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/http-client.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/symphony/pi-extension/src/http-client.ts apps/symphony/pi-extension/src/http-client.test.ts
git commit -m "feat(pi-symphony): add wave 4 http contracts"
```

---

### Task 2: Add event stream filters and persisted console filters

**Files:**
- Modify: `apps/symphony/pi-extension/src/event-stream.ts`
- Modify: `apps/symphony/pi-extension/src/event-stream.test.ts`
- Modify: `apps/symphony/pi-extension/src/state.ts`
- Modify: `apps/symphony/pi-extension/src/state.test.ts`

- [ ] **Step 1: Write failing event stream tests**

Add this test to `apps/symphony/pi-extension/src/event-stream.test.ts`.

```ts
  it("adds encoded event stream filters to the websocket URL", () => {
    expect(eventStreamUrl("http://127.0.0.1:8787/dashboard", {
      issue: "SIM-123,SIM-456",
      type: "worker,shared_context_written",
      severity: "warn,error",
    })).toBe("ws://127.0.0.1:8787/dashboard/api/v1/events?issue=SIM-123%2CSIM-456&type=worker%2Cshared_context_written&severity=warn%2Cerror");
  });
```

- [ ] **Step 2: Write failing state tests**

Add this test to `apps/symphony/pi-extension/src/state.test.ts`.

```ts
  it("persists console scope and event filters", () => {
    const state = createDefaultState();
    state.console.contextScopeFilter = "milestone:M001";
    state.console.eventFilters = { issue: "SIM-123", type: "worker", severity: "error" };

    const restored = restoreStateFromEntries([
      { type: "custom", customType: STATE_ENTRY_TYPE, data: snapshotStateForPersistence(state) },
    ]);

    expect(restored.console.contextScopeFilter).toBe("milestone:M001");
    expect(restored.console.eventFilters).toEqual({ issue: "SIM-123", type: "worker", severity: "error" });
  });
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/event-stream.test.ts src/state.test.ts
```

Expected: FAIL because event filters and persisted console filter fields do not exist.

- [ ] **Step 4: Implement event stream filters**

In `apps/symphony/pi-extension/src/event-stream.ts`, add the filter type and update options.

```ts
export interface EventStreamFilters {
  issue?: string;
  type?: string;
  severity?: string;
}

export interface EventStreamOptions {
  baseUrl: string;
  filters?: EventStreamFilters;
  onEvent: (event: SymphonyEventEnvelope) => void;
  onError: (error: Error) => void;
}
```

Replace `eventStreamUrl(baseUrl: string): string` with this implementation.

```ts
export function eventStreamUrl(baseUrl: string, filters: EventStreamFilters = {}): string {
  const url = new URL(baseUrl);
  if (url.protocol === "https:" || url.protocol === "wss:") {
    url.protocol = "wss:";
  } else if (url.protocol === "http:" || url.protocol === "ws:") {
    url.protocol = "ws:";
  } else {
    throw new Error(`Unsupported Symphony base URL protocol: ${url.protocol}`);
  }
  url.pathname = `${url.pathname.replace(/\/+$/, "")}/api/v1/events`;
  url.search = "";
  url.hash = "";
  for (const [key, value] of Object.entries(filters)) {
    const trimmed = value?.trim();
    if (trimmed) url.searchParams.set(key, trimmed);
  }
  return url.toString();
}
```

Update the websocket constructor inside `startSymphonyEventStream(...)`.

```ts
    socket = new WebSocket(eventStreamUrl(options.baseUrl, options.filters));
```

- [ ] **Step 5: Persist console filters**

In `apps/symphony/pi-extension/src/state.ts`, add this interface before `ExtensionState`.

```ts
export interface ConsoleEventFilters {
  issue?: string;
  type?: string;
  severity?: string;
}
```

Replace the `console` field in `ExtensionState` with this shape.

```ts
  console: {
    showDetails: boolean;
    contextScopeFilter?: string;
    eventFilters: ConsoleEventFilters;
  };
```

Replace `createDefaultState()` with this implementation.

```ts
export function createDefaultState(): ExtensionState {
  return {
    console: { showDetails: true, eventFilters: {} },
    stopOwnedOnShutdown: true,
  };
}
```

In `restoreStateFromSnapshot(...)`, replace the console restore block with this code.

```ts
  const restoredConsole = isRecord(data.console) ? data.console : isRecord(data.dashboard) ? data.dashboard : undefined;
  if (restoredConsole) {
    if (typeof restoredConsole.showDetails === "boolean") state.console.showDetails = restoredConsole.showDetails;
    if (typeof restoredConsole.contextScopeFilter === "string" && restoredConsole.contextScopeFilter.trim()) {
      state.console.contextScopeFilter = restoredConsole.contextScopeFilter.trim();
    }
    if (isRecord(restoredConsole.eventFilters)) {
      state.console.eventFilters = restoreEventFilters(restoredConsole.eventFilters);
    }
  }
```

Add this helper before `isRecord(...)`.

```ts
function restoreEventFilters(value: Record<string, unknown>): ConsoleEventFilters {
  const filters: ConsoleEventFilters = {};
  if (typeof value.issue === "string" && value.issue.trim()) filters.issue = value.issue.trim();
  if (typeof value.type === "string" && value.type.trim()) filters.type = value.type.trim();
  if (typeof value.severity === "string" && value.severity.trim()) filters.severity = value.severity.trim();
  return filters;
}
```

- [ ] **Step 6: Run filter tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/event-stream.test.ts src/state.test.ts
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add apps/symphony/pi-extension/src/event-stream.ts apps/symphony/pi-extension/src/event-stream.test.ts apps/symphony/pi-extension/src/state.ts apps/symphony/pi-extension/src/state.test.ts
git commit -m "feat(pi-symphony): persist wave 4 console filters"
```

---

### Task 3: Add runtime shared context operations and event counters

**Files:**
- Modify: `apps/symphony/pi-extension/src/runtime.ts`
- Modify: `apps/symphony/pi-extension/src/runtime.test.ts`

- [ ] **Step 1: Write failing runtime tests**

Add these tests to `apps/symphony/pi-extension/src/runtime.test.ts`.

```ts
  it("refreshes shared context with the persisted scope filter", async () => {
    const runtime = new SymphonyRuntime();
    runtime.client = {
      getContext: vi.fn(async () => ({
        entries: [{ id: "ctx-1", author_issue: "SIM-123", scope: { type: "project" }, content: "Decision: test", created_at: "2026-05-17T12:00:00Z", ttl_ms: 60000 }],
        summary: { total_entries: 1, entries_by_scope: { project: 1 }, oldest_entry_at: "2026-05-17T12:00:00Z", newest_entry_at: "2026-05-17T12:00:00Z" },
      })),
    } as unknown as SymphonyHttpClient;
    runtime.state.console.contextScopeFilter = "project";

    await runtime.refreshSharedContext();

    expect(runtime.client.getContext).toHaveBeenCalledWith("project", undefined);
    expect(runtime.lastContext?.entries).toHaveLength(1);
  });

  it("creates shared context then refreshes context and state", async () => {
    const runtime = new SymphonyRuntime();
    runtime.client = {
      createContext: vi.fn(async () => ({ id: "ctx-2", created_at: "2026-05-17T12:01:00Z" })),
      getContext: vi.fn(async () => ({ entries: [], summary: { total_entries: 0, entries_by_scope: {}, oldest_entry_at: null, newest_entry_at: null } })),
      getState: vi.fn(async () => ({
        running: {}, retry_queue: [], blocked: [], completed: [], pending_escalations: [],
        shared_context: { total_entries: 0, entries_by_scope: {}, oldest_entry_at: null, newest_entry_at: null },
        supervisor: { active: true, steers_issued: 0, conflicts_detected: 0, patterns_detected: 0, escalations_created: 0 },
        codex_totals: { input_tokens: 0, output_tokens: 0, total_tokens: 0, event_count: 0, seconds_running: 0 },
        codex_rate_limits: null,
        polling: { checking: false, next_poll_in_ms: 1000, poll_interval_ms: 30000 },
      })),
      toHealthSummary: vi.fn(() => ({ baseUrl: "http://127.0.0.1:8787", runningCount: 0, retryCount: 0, blockedCount: 0, completedCount: 0, pollingChecking: false, nextPollInMs: 1000, updatedAt: "2026-05-17T12:00:00Z" })),
    } as unknown as SymphonyHttpClient;

    await runtime.createSharedContext({ authorIssue: "SIM-123", scope: "project", content: "Decision: test" });

    expect(runtime.client.createContext).toHaveBeenCalledWith({ authorIssue: "SIM-123", scope: "project", content: "Decision: test" }, undefined);
    expect(runtime.client.getContext).toHaveBeenCalled();
    expect(runtime.client.getState).toHaveBeenCalled();
  });

  it("tracks event stream counters for retained event kinds", () => {
    const runtime = new SymphonyRuntime();

    runtime.recordEvent({ version: "v1", sequence: 1, timestamp: "2026-05-17T12:00:00Z", kind: "shared_context_written", severity: "info", issue: "SIM-123", event: "shared_context_written", payload: {} });
    runtime.recordEvent({ version: "v1", sequence: 2, timestamp: "2026-05-17T12:01:00Z", kind: "supervisor_conflict_detected", severity: "warn", issue: "SIM-456", event: "supervisor_conflict_detected", payload: {} });

    expect(runtime.eventStreamStats.totalEvents).toBe(2);
    expect(runtime.eventStreamStats.byKind).toEqual({ shared_context_written: 1, supervisor_conflict_detected: 1 });
    expect(runtime.eventStreamStats.bySeverity).toEqual({ info: 1, warn: 1 });
    expect(runtime.recentEvents.map((event) => event.kind)).toEqual(["shared_context_written", "supervisor_conflict_detected"]);
  });
```

Add the missing import at the top of the test file.

```ts
import { SymphonyHttpClient } from "./http-client.ts";
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/runtime.test.ts
```

Expected: FAIL because runtime shared context methods and event stream stats do not exist.

- [ ] **Step 3: Implement runtime state and methods**

In `apps/symphony/pi-extension/src/runtime.ts`, update the import from `http-client.ts`.

```ts
import {
  SymphonyHttpClient,
  type EscalationRespondResponse,
  type SharedContextCreateInput,
  type SharedContextDeleteResponse,
  type SharedContextListResponse,
  type SharedContextWriteResponse,
  type SteerResponse,
  type SymphonyEventEnvelope,
  type SymphonyStateResponse,
} from "./http-client.ts";
```

Add this interface above `export class SymphonyRuntime`.

```ts
export interface EventStreamStats {
  totalEvents: number;
  byKind: Record<string, number>;
  bySeverity: Record<string, number>;
  lastEventAt?: string;
  errorCount: number;
  lastError?: string;
}

function createEventStreamStats(): EventStreamStats {
  return { totalEvents: 0, byKind: {}, bySeverity: {}, errorCount: 0 };
}
```

Add these fields to `SymphonyRuntime`.

```ts
  lastContext?: SharedContextListResponse;
  eventStreamStats: EventStreamStats = createEventStreamStats();
```

In `attach(...)` and `clearAttachment()`, reset context and stats.

```ts
    this.lastContext = undefined;
    this.eventStreamStats = createEventStreamStats();
```

Add these methods after `respondToEscalation(...)`.

```ts
  async refreshSharedContext(signal?: AbortSignal): Promise<SharedContextListResponse> {
    if (!this.client) throw new SymphonyExtensionError("no_attachment", "No Symphony server is attached");
    const context = await this.client.getContext(this.state.console.contextScopeFilter, signal);
    this.lastContext = context;
    return context;
  }

  async createSharedContext(input: SharedContextCreateInput, signal?: AbortSignal): Promise<SharedContextWriteResponse> {
    if (!this.client) throw new SymphonyExtensionError("no_attachment", "No Symphony server is attached");
    const result = await this.client.createContext(input, signal);
    await this.refreshAfterContextMutation(signal);
    return result;
  }

  async deleteSharedContextEntry(entryId: string, signal?: AbortSignal): Promise<SharedContextDeleteResponse> {
    if (!this.client) throw new SymphonyExtensionError("no_attachment", "No Symphony server is attached");
    const result = await this.client.deleteContextEntry(entryId, signal);
    await this.refreshAfterContextMutation(signal);
    return result;
  }

  async clearSharedContextScope(signal?: AbortSignal): Promise<SharedContextDeleteResponse> {
    if (!this.client) throw new SymphonyExtensionError("no_attachment", "No Symphony server is attached");
    const result = await this.client.deleteContext(this.state.console.contextScopeFilter, signal);
    await this.refreshAfterContextMutation(signal);
    return result;
  }

  recordEventStreamError(error: Error): void {
    this.eventStreamStats.errorCount += 1;
    this.eventStreamStats.lastError = error.message;
  }

  private async refreshAfterContextMutation(signal?: AbortSignal): Promise<void> {
    await this.refreshSharedContext(signal);
    try {
      await this.refreshState(signal);
    } catch (error) {
      console.warn("Symphony state refresh failed after shared context mutation", error);
    }
  }
```

Replace `recordEvent(...)` with this implementation.

```ts
  recordEvent(event: SymphonyEventEnvelope): void {
    this.eventStreamStats.totalEvents += 1;
    this.eventStreamStats.byKind[event.kind] = (this.eventStreamStats.byKind[event.kind] ?? 0) + 1;
    this.eventStreamStats.bySeverity[event.severity] = (this.eventStreamStats.bySeverity[event.severity] ?? 0) + 1;
    this.eventStreamStats.lastEventAt = event.timestamp;

    if (!isRetainedEventKind(event.kind)) return;
    this.recentEvents = [...this.recentEvents, event].slice(-20);
  }
```

Add this helper after the class.

```ts
function isRetainedEventKind(kind: string): boolean {
  return kind === "worker" || kind === "runtime" || kind.startsWith("escalation_") || kind.startsWith("shared_context_") || kind.startsWith("supervisor_");
}
```

- [ ] **Step 4: Run runtime tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/runtime.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/symphony/pi-extension/src/runtime.ts apps/symphony/pi-extension/src/runtime.test.ts
git commit -m "feat(pi-symphony): add shared context runtime operations"
```

---

### Task 4: Build shared context and diagnostics console models

**Files:**
- Modify: `apps/symphony/pi-extension/src/console-model.ts`
- Modify: `apps/symphony/pi-extension/src/console-model.test.ts`

- [ ] **Step 1: Write failing model tests**

Add these imports to `apps/symphony/pi-extension/src/console-model.test.ts`.

```ts
import { buildDiagnosticRows, buildSharedContextRows, buildSharedContextSummaryRows } from "./console-model.ts";
import type { EventStreamStats } from "./runtime.ts";
```

If the file already imports from `console-model.ts`, merge the new symbols into the existing import.

Add these tests inside `describe("console model", () => { ... })`.

```ts
  it("builds shared context rows with scope keys and truncated content", () => {
    const rows = buildSharedContextRows({
      entries: [
        { id: "ctx-2", author_issue: "SIM-456", scope: { type: "label", value: "backend" }, content: "Backend note", created_at: "2026-05-17T12:02:00Z", ttl_ms: 120000 },
        { id: "ctx-1", author_issue: "SIM-123", scope: { type: "project" }, content: "Decision: use the existing auth module", created_at: "2026-05-17T12:00:00Z", ttl_ms: 60000 },
      ],
      summary: { total_entries: 2, entries_by_scope: { project: 1, "label:backend": 1 }, oldest_entry_at: "2026-05-17T12:00:00Z", newest_entry_at: "2026-05-17T12:02:00Z" },
    });

    expect(rows.map((row) => row.id)).toEqual(["ctx-1", "ctx-2"]);
    expect(rows[0]).toMatchObject({ authorIssue: "SIM-123", scope: "project", ttl: "1m 0s", contentPreview: "Decision: use the existing auth module" });
    expect(rows[1]).toMatchObject({ authorIssue: "SIM-456", scope: "label:backend", ttl: "2m 0s" });
  });

  it("builds shared context summary rows from state and fetched context", () => {
    const rows = buildSharedContextSummaryRows(
      { total_entries: 2, entries_by_scope: { project: 1, "milestone:M001": 1 }, oldest_entry_at: "2026-05-17T12:00:00Z", newest_entry_at: "2026-05-17T12:05:00Z" },
      "milestone:M001",
    );

    expect(rows).toEqual([
      "filter: milestone:M001",
      "entries: 2 | scopes: milestone:M001=1, project=1",
      "oldest: 2026-05-17T12:00:00Z | newest: 2026-05-17T12:05:00Z",
    ]);
  });

  it("builds diagnostics rows with polling, tokens, rate limits, supervisor, and event counters", () => {
    const stats: EventStreamStats = {
      totalEvents: 3,
      byKind: { worker: 2, shared_context_written: 1 },
      bySeverity: { info: 2, warn: 1 },
      lastEventAt: "2026-05-17T12:06:00Z",
      errorCount: 1,
      lastError: "closed with code 1006",
    };
    const state = stateFixture();
    state.shared_context = { total_entries: 1, entries_by_scope: { project: 1 }, oldest_entry_at: "2026-05-17T12:00:00Z", newest_entry_at: "2026-05-17T12:00:00Z" };
    state.supervisor = { active: true, steers_issued: 1, conflicts_detected: 2, patterns_detected: 3, escalations_created: 4 };
    state.codex_totals = { input_tokens: 1000, output_tokens: 500, total_tokens: 1500, event_count: 9, seconds_running: 90 };
    state.codex_rate_limits = { requests: { remaining: 80, limit: 100, reset_seconds: 120 } };
    state.polling = { checking: true, next_poll_in_ms: 2500, poll_interval_ms: 30000, poll_count: 7, last_poll_at: "2026-05-17T12:05:00Z" };

    expect(buildDiagnosticRows(state, stats, { issue: "SIM-123", type: "worker", severity: "warn" })).toEqual([
      "polling: checking | next: 3s | interval: 30s | count: 7 | last: 2026-05-17T12:05:00Z",
      "tokens: input 1,000 | output 500 | total 1,500 | codex events 9 | runtime 1m 30s",
      "rate: requests: 20% used (2m 0s)",
      "supervisor: active | steers 1 | conflicts 2 | patterns 3 | escalations 4",
      "event stream: received 3 | errors 1 | last 2026-05-17T12:06:00Z",
      "event filters: issue=SIM-123 type=worker severity=warn",
      "event kinds: shared_context_written=1, worker=2",
      "event severity: info=2, warn=1",
      "last stream error: closed with code 1006",
    ]);
  });
```

- [ ] **Step 2: Run model tests to verify they fail**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/console-model.test.ts
```

Expected: FAIL because shared context and diagnostics model functions do not exist.

- [ ] **Step 3: Add model imports and types**

In `apps/symphony/pi-extension/src/console-model.ts`, extend the HTTP import list.

```ts
  SharedContextListResponse,
  SharedContextSummaryResponse,
```

Add this runtime type import.

```ts
import type { EventStreamStats } from "./runtime.ts";
```

Add these interfaces after `EscalationRow`.

```ts
export interface SharedContextRow {
  id: string;
  authorIssue: string;
  scope: string;
  contentPreview: string;
  createdAt: string;
  ttl: string;
}

export interface ConsoleEventFiltersView {
  issue?: string;
  type?: string;
  severity?: string;
}
```

- [ ] **Step 4: Add shared context row builders**

Add these exported functions after `buildEscalationRows(...)`.

```ts
export function buildSharedContextRows(context: SharedContextListResponse | undefined): SharedContextRow[] {
  return (context?.entries ?? [])
    .slice()
    .sort((left, right) => timestampMs(left.created_at) - timestampMs(right.created_at) || left.id.localeCompare(right.id))
    .map((entry) => ({
      id: entry.id,
      authorIssue: entry.author_issue,
      scope: formatContextScope(entry.scope),
      contentPreview: truncateText(entry.content.replace(/\s+/g, " ").trim(), 120),
      createdAt: entry.created_at,
      ttl: formatDuration(entry.ttl_ms),
    }));
}

export function buildSharedContextSummaryRows(summary: SharedContextSummaryResponse | undefined, activeFilter: string | undefined): string[] {
  if (!summary) return ["filter: all", "entries: 0 | scopes: none", "oldest: - | newest: -"];
  const scopes = Object.entries(summary.entries_by_scope)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([scope, count]) => `${scope}=${count}`)
    .join(", ");
  return [
    `filter: ${activeFilter?.trim() || "all"}`,
    `entries: ${summary.total_entries} | scopes: ${scopes || "none"}`,
    `oldest: ${summary.oldest_entry_at ?? "-"} | newest: ${summary.newest_entry_at ?? "-"}`,
  ];
}
```

Add this helper near `formatDuration(...)`.

```ts
function formatContextScope(scope: { type: string; value?: string }): string {
  if (scope.type === "project") return "project";
  if (scope.type === "milestone" && scope.value) return `milestone:${scope.value}`;
  if (scope.type === "label" && scope.value) return `label:${scope.value}`;
  return scope.type;
}
```

- [ ] **Step 5: Add diagnostics builders**

Add this exported function after `buildSharedContextSummaryRows(...)`.

```ts
export function buildDiagnosticRows(state: SymphonyStateResponse | undefined, stats: EventStreamStats, filters: ConsoleEventFiltersView): string[] {
  const polling = state?.polling;
  const totals = state?.codex_totals;
  const supervisor = state?.supervisor;
  return [
    `polling: ${polling?.checking ? "checking" : "idle"} | next: ${formatDuration(polling?.next_poll_in_ms ?? 0)} | interval: ${formatDuration(polling?.poll_interval_ms ?? 0)} | count: ${polling?.poll_count ?? 0} | last: ${polling?.last_poll_at ?? "never"}`,
    `tokens: input ${formatInteger(totals?.input_tokens ?? 0)} | output ${formatInteger(totals?.output_tokens ?? 0)} | total ${formatInteger(totals?.total_tokens ?? 0)} | codex events ${formatInteger(totals?.event_count ?? 0)} | runtime ${formatDuration((totals?.seconds_running ?? 0) * 1000)}`,
    `rate: ${formatRateLimits(state?.codex_rate_limits)}`,
    `supervisor: ${supervisorStatus(supervisor)} | steers ${supervisor?.steers_issued ?? 0} | conflicts ${supervisor?.conflicts_detected ?? 0} | patterns ${supervisor?.patterns_detected ?? 0} | escalations ${supervisor?.escalations_created ?? 0}`,
    `event stream: received ${stats.totalEvents} | errors ${stats.errorCount} | last ${stats.lastEventAt ?? "never"}`,
    `event filters: ${formatEventFilters(filters)}`,
    `event kinds: ${formatCountMap(stats.byKind)}`,
    `event severity: ${formatCountMap(stats.bySeverity)}`,
    ...(stats.lastError ? [`last stream error: ${stats.lastError}`] : []),
  ];
}
```

Add these helpers near the bottom of the file.

```ts
function formatInteger(value: number): string {
  return new Intl.NumberFormat("en-US").format(value);
}

function supervisorStatus(value: SymphonyStateResponse["supervisor"] | undefined): string {
  if (!value) return "unknown";
  if (value.status) return value.status;
  return value.active === false ? "inactive" : "active";
}

function formatEventFilters(filters: ConsoleEventFiltersView): string {
  const parts = [
    filters.issue ? `issue=${filters.issue}` : undefined,
    filters.type ? `type=${filters.type}` : undefined,
    filters.severity ? `severity=${filters.severity}` : undefined,
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(" ") : "none";
}

function formatCountMap(counts: Record<string, number>): string {
  const parts = Object.entries(counts)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, count]) => `${key}=${count}`);
  return parts.length > 0 ? parts.join(", ") : "none";
}

function formatRateLimits(value: unknown): string {
  const summaries = rateLimitSummaries(value);
  return summaries.length > 0 ? summaries.join("  ") : "n/a";
}

function rateLimitSummaries(value: unknown): string[] {
  if (!isRecord(value)) return [];
  const nested = Object.entries(value)
    .filter(([name]) => name !== "limit_id" && name !== "limit_name")
    .flatMap(([name, bucket]) => (isRecord(bucket) ? bucketUsageText(name, bucket) : []));
  if (nested.length > 0) return nested.sort();
  const limitName = typeof value.limit_name === "string" ? value.limit_name : "limit";
  const fallback = bucketUsageText(limitName, value);
  return fallback ? [fallback] : [];
}

function bucketUsageText(label: string, bucket: Record<string, unknown>): string | undefined {
  const remaining = typeof bucket.remaining === "number" ? bucket.remaining : undefined;
  const limit = typeof bucket.limit === "number" ? bucket.limit : undefined;
  if (remaining === undefined || limit === undefined || limit <= 0) return undefined;
  const usedPct = Math.round(Math.max(0, Math.min(100, (1 - remaining / limit) * 100)));
  const window = typeof bucket.reset_seconds === "number" ? formatDuration(bucket.reset_seconds * 1000) : "-";
  return `${label}: ${usedPct}% used (${window})`;
}
```

- [ ] **Step 6: Run model tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/console-model.test.ts
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add apps/symphony/pi-extension/src/console-model.ts apps/symphony/pi-extension/src/console-model.test.ts
git commit -m "feat(pi-symphony): model wave 4 console diagnostics"
```

---

### Task 5: Render shared context and support create, delete, and scope filtering

**Files:**
- Modify: `apps/symphony/pi-extension/src/console.ts`
- Modify: `apps/symphony/pi-extension/src/console.test.ts`

- [ ] **Step 1: Write failing console tests for shared context rendering and operations**

In `apps/symphony/pi-extension/src/console.test.ts`, add this helper after `workerStateFixture()`.

```ts
function sharedContextFixture() {
  return {
    entries: [
      { id: "ctx-1", author_issue: "SIM-123", scope: { type: "project" as const }, content: "Decision: use existing auth module", created_at: "2026-05-17T12:00:00Z", ttl_ms: 60000 },
      { id: "ctx-2", author_issue: "SIM-456", scope: { type: "label" as const, value: "backend" }, content: "Backend worker owns API validation", created_at: "2026-05-17T12:02:00Z", ttl_ms: 120000 },
    ],
    summary: { total_entries: 2, entries_by_scope: { project: 1, "label:backend": 1 }, oldest_entry_at: "2026-05-17T12:00:00Z", newest_entry_at: "2026-05-17T12:02:00Z" },
  };
}
```

Update every `new SymphonyConsoleComponent({ ... })` test object by adding these default options when absent.

```ts
      getSharedContext: () => undefined,
      getEventStreamStats: () => ({ totalEvents: 0, byKind: {}, bySeverity: {}, errorCount: 0 }),
      createSharedContext: async () => undefined,
      deleteSharedContextEntry: async () => undefined,
      clearSharedContextScope: async () => undefined,
      setContextScopeFilter: () => undefined,
      setEventFilters: async () => undefined,
```

Add these tests inside `describe("SymphonyConsoleComponent", () => { ... })`.

```ts
  it("renders shared context summary, rows, and empty filter state", () => {
    const state = createDefaultState();
    const consoleComponent = new SymphonyConsoleComponent({
      state,
      getState: () => workerStateFixture(),
      getSharedContext: () => sharedContextFixture(),
      getEventStreamStats: () => ({ totalEvents: 0, byKind: {}, bySeverity: {}, errorCount: 0 }),
      getEvents: () => [],
      refresh: async () => undefined,
      steer: async () => undefined,
      respondToEscalation: async () => undefined,
      createSharedContext: async () => undefined,
      deleteSharedContextEntry: async () => undefined,
      clearSharedContextScope: async () => undefined,
      setContextScopeFilter: () => undefined,
      setEventFilters: async () => undefined,
      prompt: async () => undefined,
      close: () => undefined,
      requestRender: () => undefined,
      notify: () => undefined,
    });

    const output = consoleComponent.render(220).join("\n");

    expect(output).toContain("Shared Context");
    expect(output).toContain("filter: all");
    expect(output).toContain("entries: 2 | scopes: label:backend=1, project=1");
    expect(output).toContain("ctx-1");
    expect(output).toContain("SIM-123");
    expect(output).toContain("Decision: use existing auth module");
  });

  it("creates shared context from pipe-delimited prompt input", async () => {
    const createSharedContext = vi.fn(async () => undefined);
    const notify = vi.fn();
    const consoleComponent = new SymphonyConsoleComponent({
      state: createDefaultState(),
      getState: () => workerStateFixture(),
      getSharedContext: () => sharedContextFixture(),
      getEventStreamStats: () => ({ totalEvents: 0, byKind: {}, bySeverity: {}, errorCount: 0 }),
      getEvents: () => [],
      refresh: async () => undefined,
      steer: async () => undefined,
      respondToEscalation: async () => undefined,
      createSharedContext,
      deleteSharedContextEntry: async () => undefined,
      clearSharedContextScope: async () => undefined,
      setContextScopeFilter: () => undefined,
      setEventFilters: async () => undefined,
      prompt: async () => "SIM-123 | project | Decision: reuse auth | 60000",
      close: () => undefined,
      requestRender: () => undefined,
      notify,
    });

    consoleComponent.handleInput("c");
    await expect.poll(() => createSharedContext.mock.calls.length, { interval: 10, timeout: 1000 }).toBe(1);

    expect(createSharedContext).toHaveBeenCalledWith({ authorIssue: "SIM-123", scope: "project", content: "Decision: reuse auth", ttlMs: 60000 });
    expect(notify).toHaveBeenCalledWith("Shared context entry created", "info");
  });

  it("deletes the selected shared context row", async () => {
    const deleteSharedContextEntry = vi.fn(async () => ({ deleted: 1 }));
    const notify = vi.fn();
    const consoleComponent = new SymphonyConsoleComponent({
      state: createDefaultState(),
      getState: () => workerStateFixture(),
      getSharedContext: () => sharedContextFixture(),
      getEventStreamStats: () => ({ totalEvents: 0, byKind: {}, bySeverity: {}, errorCount: 0 }),
      getEvents: () => [],
      refresh: async () => undefined,
      steer: async () => undefined,
      respondToEscalation: async () => undefined,
      createSharedContext: async () => undefined,
      deleteSharedContextEntry,
      clearSharedContextScope: async () => undefined,
      setContextScopeFilter: () => undefined,
      setEventFilters: async () => undefined,
      prompt: async () => undefined,
      close: () => undefined,
      requestRender: () => undefined,
      notify,
    });

    for (let index = 0; index < 6; index += 1) consoleComponent.handleInput("\u001b[B");
    consoleComponent.handleInput("x");
    await expect.poll(() => deleteSharedContextEntry.mock.calls.length, { interval: 10, timeout: 1000 }).toBe(1);

    expect(deleteSharedContextEntry).toHaveBeenCalledWith("ctx-1");
    expect(notify).toHaveBeenCalledWith("Deleted shared context entry ctx-1", "info");
  });

  it("sets and clears the shared context scope filter", async () => {
    const state = createDefaultState();
    const setContextScopeFilter = vi.fn();
    const consoleComponent = new SymphonyConsoleComponent({
      state,
      getState: () => workerStateFixture(),
      getSharedContext: () => sharedContextFixture(),
      getEventStreamStats: () => ({ totalEvents: 0, byKind: {}, bySeverity: {}, errorCount: 0 }),
      getEvents: () => [],
      refresh: async () => undefined,
      steer: async () => undefined,
      respondToEscalation: async () => undefined,
      createSharedContext: async () => undefined,
      deleteSharedContextEntry: async () => undefined,
      clearSharedContextScope: async () => undefined,
      setContextScopeFilter,
      setEventFilters: async () => undefined,
      prompt: async () => "label:backend",
      close: () => undefined,
      requestRender: () => undefined,
      notify: () => undefined,
    });

    consoleComponent.handleInput("f");
    await expect.poll(() => setContextScopeFilter.mock.calls.length, { interval: 10, timeout: 1000 }).toBe(1);

    expect(setContextScopeFilter).toHaveBeenCalledWith("label:backend");
  });
```

- [ ] **Step 2: Run console tests to verify they fail**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/console.test.ts
```

Expected: FAIL because console shared context options and rendering do not exist.

- [ ] **Step 3: Extend console options and imports**

In `apps/symphony/pi-extension/src/console.ts`, replace the model import with this import.

```ts
import {
  buildDiagnosticRows,
  buildEscalationRows,
  buildIssueRows,
  buildSharedContextRows,
  buildSharedContextSummaryRows,
  buildWorkerRows,
  formatEventRows,
  type EscalationRow,
  type IssueRow,
  type SharedContextRow,
  type WorkerRow,
} from "./console-model.ts";
```

Update HTTP and runtime imports.

```ts
import type { SharedContextCreateInput, SharedContextListResponse, SymphonyEventEnvelope, SymphonyStateResponse } from "./http-client.ts";
import type { EventStreamStats, SymphonyRuntime } from "./runtime.ts";
```

Extend `ConsoleShortcutAction`.

```ts
export type ConsoleShortcutAction = "selectPrevious" | "selectNext" | "refresh" | "steer" | "respondEscalation" | "createContext" | "deleteContext" | "filterContext" | "filterEvents" | "toggleDetails" | "close";
```

Add cases to `handleActiveConsoleShortcut(...)`.

```ts
    case "createContext":
      await activeConsole.createContextNow();
      return;
    case "deleteContext":
      await activeConsole.deleteSelectedContextNow();
      return;
    case "filterContext":
      await activeConsole.filterContextNow();
      return;
    case "filterEvents":
      await activeConsole.filterEventsNow();
      return;
```

Extend `ConsoleOptions`.

```ts
  getSharedContext: () => SharedContextListResponse | undefined;
  getEventStreamStats: () => EventStreamStats;
  createSharedContext: (input: SharedContextCreateInput) => Promise<void>;
  deleteSharedContextEntry: (entryId: string) => Promise<{ deleted: number }>;
  clearSharedContextScope: () => Promise<{ deleted: number }>;
  setContextScopeFilter: (scope: string | undefined) => Promise<void> | void;
  setEventFilters: (filters: { issue?: string; type?: string; severity?: string }) => Promise<void> | void;
```

- [ ] **Step 4: Add key handling and public methods**

In `handleInput(...)`, add these blocks before arrow-key handling.

```ts
    if (data === "c" || data === "C") {
      void this.createContextNow();
      return;
    }

    if (data === "x" || data === "X") {
      void this.deleteSelectedContextNow();
      return;
    }

    if (data === "f" || data === "F") {
      void this.filterContextNow();
      return;
    }

    if (data === "v" || data === "V") {
      void this.filterEventsNow();
      return;
    }
```

Add these public methods after `respondToEscalationNow()`.

```ts
  async createContextNow(): Promise<void> {
    await this.createContextEntry();
  }

  async deleteSelectedContextNow(): Promise<void> {
    await this.deleteSelectedContext();
  }

  async filterContextNow(): Promise<void> {
    await this.updateContextFilter();
  }

  async filterEventsNow(): Promise<void> {
    await this.updateEventFilters();
  }
```

- [ ] **Step 5: Include context rows in selection and render context sections**

Inside `render(width: number)`, add these values after `escalationRows`.

```ts
    const sharedContext = this.options.getSharedContext();
    const contextRows: SharedContextRow[] = buildSharedContextRows(sharedContext);
    const totalSelectableRows = issueRows.length + escalationRows.length + contextRows.length;
    this.clampSelection(totalSelectableRows);
```

Replace the existing `this.clampSelection(issueRows.length + escalationRows.length);` line with the new total selectable rows block.

Add these sections before `Events`.

```ts
      ...boxLines("Shared Context", renderSharedContextSection(
        buildSharedContextSummaryRows(sharedContext?.summary ?? symphonyState?.shared_context, state.console.contextScopeFilter),
        contextRows,
        this.selectedIndex - issueRows.length - escalationRows.length,
        theme,
      ), consoleWidth, theme),
      ...boxLines("Diagnostics", buildDiagnosticRows(symphonyState, this.options.getEventStreamStats(), state.console.eventFilters), consoleWidth, theme),
```

Update `moveSelection(...)`, `steerSelectedWorker(...)`, and `respondToSelectedEscalation(...)` so each uses this total count.

```ts
    const rowCount = buildIssueRows(symphonyState).length + buildEscalationRows(symphonyState).length + buildSharedContextRows(this.options.getSharedContext()).length;
```

- [ ] **Step 6: Add shared context operations and parsers**

Add these private methods inside `SymphonyConsoleComponent` before `refresh()`.

```ts
  private selectedContextRow(): SharedContextRow | undefined {
    const symphonyState = this.options.getState();
    const issueRows = buildIssueRows(symphonyState);
    const escalationRows = buildEscalationRows(symphonyState);
    const contextRows = buildSharedContextRows(this.options.getSharedContext());
    this.clampSelection(issueRows.length + escalationRows.length + contextRows.length);
    return contextRows[this.selectedIndex - issueRows.length - escalationRows.length];
  }

  private async createContextEntry(): Promise<void> {
    try {
      const value = await this.options.prompt("Create Symphony shared context", "author_issue | scope | content | ttl_ms(optional), or JSON");
      if (!value?.trim()) return;
      await this.options.createSharedContext(parseContextCreateInput(value));
      this.options.notify("Shared context entry created", "info");
    } catch (error) {
      this.options.notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      this.options.requestRender();
    }
  }

  private async deleteSelectedContext(): Promise<void> {
    const row = this.selectedContextRow();
    if (!row) {
      this.options.notify("Select a shared context entry before deleting", "warning");
      return;
    }
    try {
      await this.options.deleteSharedContextEntry(row.id);
      this.options.notify(`Deleted shared context entry ${row.id}`, "info");
    } catch (error) {
      this.options.notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      this.options.requestRender();
    }
  }

  private async updateContextFilter(): Promise<void> {
    try {
      const value = await this.options.prompt("Filter Symphony shared context", "scope filter: project, milestone:<id>, label:<name>, or blank for all");
      const scope = value?.trim() || undefined;
      await this.options.setContextScopeFilter(scope);
      this.options.notify(scope ? `Shared context filter set to ${scope}` : "Shared context filter cleared", "info");
    } catch (error) {
      this.options.notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      this.options.requestRender();
    }
  }

  private async updateEventFilters(): Promise<void> {
    try {
      const value = await this.options.prompt("Filter Symphony events", "issue=SIM-123 type=worker severity=warn, or blank for all");
      await this.options.setEventFilters(parseEventFilterInput(value ?? ""));
      this.options.notify(value?.trim() ? "Event filters updated" : "Event filters cleared", "info");
    } catch (error) {
      this.options.notify(error instanceof Error ? error.message : String(error), "error");
    } finally {
      this.options.requestRender();
    }
  }
```

Add these functions below `parseEscalationResponseInput(...)`.

```ts
function parseContextCreateInput(value: string): SharedContextCreateInput {
  const trimmed = value.trim();
  if (trimmed.startsWith("{")) {
    const parsed = JSON.parse(trimmed) as Record<string, unknown>;
    const input = {
      authorIssue: parsed.authorIssue ?? parsed.author_issue,
      scope: parsed.scope,
      content: parsed.content,
      ttlMs: parsed.ttlMs ?? parsed.ttl_ms,
    };
    return validateContextCreateInput(input);
  }

  const parts = value.split("|").map((part) => part.trim());
  return validateContextCreateInput({ authorIssue: parts[0], scope: parts[1], content: parts[2], ttlMs: parts[3] ? Number(parts[3]) : undefined });
}

function validateContextCreateInput(value: Record<string, unknown>): SharedContextCreateInput {
  if (typeof value.authorIssue !== "string" || !value.authorIssue.trim()) throw new Error("Shared context author issue is required");
  if (typeof value.scope !== "string" || !value.scope.trim()) throw new Error("Shared context scope is required");
  if (typeof value.content !== "string" || !value.content.trim()) throw new Error("Shared context content is required");
  if (value.ttlMs !== undefined && (!Number.isFinite(value.ttlMs) || Number(value.ttlMs) <= 0)) throw new Error("Shared context ttl_ms must be a positive number");
  return {
    authorIssue: value.authorIssue.trim(),
    scope: value.scope.trim(),
    content: value.content.trim(),
    ttlMs: value.ttlMs === undefined ? undefined : Number(value.ttlMs),
  };
}

function parseEventFilterInput(value: string): { issue?: string; type?: string; severity?: string } {
  const filters: { issue?: string; type?: string; severity?: string } = {};
  for (const token of value.trim().split(/\s+/).filter(Boolean)) {
    const [key, ...rest] = token.split("=");
    const filterValue = rest.join("=").trim();
    if (!filterValue) throw new Error(`Event filter ${key} must have a value`);
    if (key === "issue" || key === "type" || key === "severity") {
      filters[key] = filterValue;
    } else {
      throw new Error(`Unsupported event filter ${key}. Use issue, type, or severity`);
    }
  }
  return filters;
}
```

- [ ] **Step 7: Add render helper and update actions**

Add this helper after `renderSelectedEscalationDetails(...)`.

```ts
function renderSharedContextSection(summaryRows: string[], rows: SharedContextRow[], selectedIndex: number, theme?: ConsoleTheme): string[] {
  const lines = [...summaryRows, color(theme, "dim", "sel id       author   scope           ttl      content")];
  if (rows.length === 0) return [...lines, color(theme, "dim", "-   no shared context entries")];

  for (const [index, row] of rows.entries()) {
    const selected = index === selectedIndex ? ">" : " ";
    const line = [selected, pad(row.id, 8), pad(row.authorIssue, 8), pad(row.scope, 15), pad(row.ttl, 8), row.contentPreview].join(" ");
    lines.push(index === selectedIndex ? selectedLine(theme, line) : line);
  }
  return lines;
}
```

Replace `renderActionLegend(...)` with this implementation.

```ts
function renderActionLegend(refreshing: boolean, width: number, theme?: ConsoleTheme): string[] {
  const keyboard = "Keyboard: ↑/↓ select | r refresh | s steer | e escalation | c context | x delete context | f context filter | v event filter | d details | q close";
  const shortcuts = "Shortcuts: ctrl+shift+↑/↓ select | ctrl+shift+r refresh | ctrl+shift+t steer | ctrl+shift+e escalation | ctrl+shift+c context | ctrl+shift+x delete | ctrl+shift+f filters | ctrl+shift+i details | ctrl+shift+q close";
  const commands = "Commands: /symphony:refresh | /symphony:status | /symphony:stop";
  if (refreshing) return [color(theme, "warning", "refreshing..."), commands];
  if (visibleLength(keyboard) <= width - 4) return [keyboard, shortcuts, commands];
  return [
    "Keyboard: ↑/↓ select | r refresh | s steer | e escalation | c context | x delete context",
    "          f context filter | v event filter | d details | q/Escape close",
    shortcuts,
    commands,
  ];
}
```

- [ ] **Step 8: Wire runtime in `openConsole(...)`**

Inside `openConsole(...)`, after `await runtime.refreshState();`, add:

```ts
    await runtime.refreshSharedContext();
```

If refresh of context fails, catch it in the existing `try` block and notify through the existing error path.

Update `startSymphonyEventStream(...)` call to pass filters and record errors.

```ts
      eventStream = startSymphonyEventStream({
        baseUrl: runtime.state.attachedBaseUrl,
        filters: runtime.state.console.eventFilters,
        onEvent: (event) => {
          runtime.recordEvent(event);
          tui.requestRender();
          scheduleLiveRefresh();
        },
        onError: (error) => {
          runtime.recordEventStreamError(error);
          if (lastEventStreamErrorMessage === error.message) return;
          lastEventStreamErrorMessage = error.message;
          ctx.ui.notify(`Symphony event stream unavailable: ${error.message}`, "warning");
        },
      });
```

Add these options to the `new SymphonyConsoleComponent({ ... })` call.

```ts
      getSharedContext: () => runtime.lastContext,
      getEventStreamStats: () => runtime.eventStreamStats,
      createSharedContext: async (input) => {
        await runtime.createSharedContext(input);
      },
      deleteSharedContextEntry: async (entryId) => {
        return await runtime.deleteSharedContextEntry(entryId);
      },
      clearSharedContextScope: async () => {
        return await runtime.clearSharedContextScope();
      },
      setContextScopeFilter: async (scope) => {
        runtime.state.console.contextScopeFilter = scope;
        await runtime.refreshSharedContext();
      },
      setEventFilters: async (filters) => {
        runtime.state.console.eventFilters = filters;
        eventStream?.close();
        eventStream = undefined;
        if (runtime.state.attachedBaseUrl) {
          eventStream = startSymphonyEventStream({
            baseUrl: runtime.state.attachedBaseUrl,
            filters: runtime.state.console.eventFilters,
            onEvent: (event) => {
              runtime.recordEvent(event);
              tui.requestRender();
              scheduleLiveRefresh();
            },
            onError: (error) => {
              runtime.recordEventStreamError(error);
              if (lastEventStreamErrorMessage === error.message) return;
              lastEventStreamErrorMessage = error.message;
              ctx.ui.notify(`Symphony event stream unavailable: ${error.message}`, "warning");
            },
          });
        }
      },
```

- [ ] **Step 9: Run console tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/console.test.ts
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add apps/symphony/pi-extension/src/console.ts apps/symphony/pi-extension/src/console.test.ts
git commit -m "feat(pi-symphony): render shared context console controls"
```

---

### Task 6: Polish diagnostics, help text, and shortcuts

**Files:**
- Modify: `apps/symphony/pi-extension/src/commands.ts`
- Modify: `apps/symphony/pi-extension/src/commands.test.ts`
- Modify: `apps/symphony/pi-extension/src/console.test.ts`

- [ ] **Step 1: Write failing command tests**

In `apps/symphony/pi-extension/src/commands.test.ts`, add expectations to the shortcut registration test for these shortcuts.

```ts
    expect(pi.registerShortcut).toHaveBeenCalledWith("ctrl+shift+c", expect.objectContaining({ description: "Create Symphony shared context" }));
    expect(pi.registerShortcut).toHaveBeenCalledWith("ctrl+shift+x", expect.objectContaining({ description: "Delete selected Symphony shared context" }));
    expect(pi.registerShortcut).toHaveBeenCalledWith("ctrl+shift+f", expect.objectContaining({ description: "Filter Symphony console context and events" }));
```

Add help-text expectations to the help command test.

```ts
    expect(notify).toHaveBeenCalledWith(expect.stringContaining("c create shared context"), "info");
    expect(notify).toHaveBeenCalledWith(expect.stringContaining("x delete selected shared context"), "info");
    expect(notify).toHaveBeenCalledWith(expect.stringContaining("f filter shared context, v filter events"), "info");
```

- [ ] **Step 2: Run command tests to verify they fail**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/commands.test.ts
```

Expected: FAIL because new shortcuts and help text are missing.

- [ ] **Step 3: Register shortcuts**

In `apps/symphony/pi-extension/src/commands.ts`, add these entries to the `shortcuts` array in `registerConsoleShortcuts(...)` after the escalation shortcut.

```ts
    { key: "ctrl+shift+c", action: "createContext", description: "Create Symphony shared context" },
    { key: "ctrl+shift+x", action: "deleteContext", description: "Delete selected Symphony shared context" },
    { key: "ctrl+shift+f", action: "filterContext", description: "Filter Symphony console context and events" },
```

- [ ] **Step 4: Update help text**

Replace the console key lines in `helpText(...)` with these lines.

```ts
    "Console keys:",
    "↑/↓ select items",
    "r refresh, s steer selected running worker, e respond to selected escalation",
    "c create shared context, x delete selected shared context",
    "f filter shared context, v filter events",
    "d details, q/Escape close",
```

- [ ] **Step 5: Run command and console tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/commands.test.ts src/console.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/symphony/pi-extension/src/commands.ts apps/symphony/pi-extension/src/commands.test.ts apps/symphony/pi-extension/src/console.test.ts
git commit -m "feat(pi-symphony): polish wave 4 console help"
```

---

### Task 7: Add a Wave 4 mock server

**Files:**
- Create: `apps/symphony/pi-extension/scripts/wave4-mock-server.mjs`
- Create: `apps/symphony/pi-extension/src/wave4-mock-server.test.ts`
- Modify: `apps/symphony/pi-extension/package.json`

- [ ] **Step 1: Write failing mock server test**

Create `apps/symphony/pi-extension/src/wave4-mock-server.test.ts` with this content.

```ts
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { afterEach, describe, expect, it } from "vitest";

let child: ChildProcessWithoutNullStreams | undefined;

afterEach(() => {
  child?.kill();
  child = undefined;
});

describe("wave4 mock server script", () => {
  it("serves Wave 4 diagnostics and shared context CRUD", async () => {
    const baseUrl = await startMockServer();

    const stateResponse = await fetch(`${baseUrl}/api/v1/state`);
    const state = await stateResponse.json() as Record<string, unknown>;
    expect(stateResponse.status).toBe(200);
    expect(state).toMatchObject({
      shared_context: { total_entries: 2, entries_by_scope: { project: 1, "label:backend": 1 } },
      supervisor: { active: true, steers_issued: 1, conflicts_detected: 1, patterns_detected: 1, escalations_created: 1 },
      codex_totals: { input_tokens: 800, output_tokens: 400, total_tokens: 1200, event_count: 2, seconds_running: 60 },
      codex_rate_limits: { requests: { remaining: 80, limit: 100, reset_seconds: 120 } },
    });

    const contextResponse = await fetch(`${baseUrl}/api/v1/context?scope=label%3Abackend`);
    const context = await contextResponse.json() as Record<string, unknown>;
    expect(contextResponse.status).toBe(200);
    expect(context).toMatchObject({ entries: [expect.objectContaining({ id: "ctx-2", author_issue: "SIM-456" })] });

    const createResponse = await fetch(`${baseUrl}/api/v1/context`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ author_issue: "SIM-789", scope: "project", content: "Decision: new context", ttl_ms: 60000 }),
    });
    await expect(createResponse.json()).resolves.toMatchObject({ id: expect.stringMatching(/^ctx-/) });

    const afterCreate = await fetch(`${baseUrl}/api/v1/context`);
    const afterCreateBody = await afterCreate.json() as { entries: unknown[] };
    expect(afterCreateBody.entries).toHaveLength(3);

    const deleteResponse = await fetch(`${baseUrl}/api/v1/context/ctx-1`, { method: "DELETE" });
    await expect(deleteResponse.json()).resolves.toEqual({ deleted: 1 });

    const clearResponse = await fetch(`${baseUrl}/api/v1/context?scope=label%3Abackend`, { method: "DELETE" });
    await expect(clearResponse.json()).resolves.toEqual({ deleted: 1 });
  });
});

async function startMockServer(): Promise<string> {
  child = spawn(process.execPath, ["scripts/wave4-mock-server.mjs", "--port", "0"], {
    cwd: new URL("..", import.meta.url),
    env: { ...process.env, NO_COLOR: "1" },
  });

  let stderr = "";
  let stdout = "";
  child.stderr.on("data", (chunk) => {
    stderr += String(chunk);
  });

  return await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error(`mock server did not start. stderr: ${stderr}`));
    }, 5000);

    child?.stdout.on("data", (chunk) => {
      stdout = (stdout + String(chunk)).slice(-2000);
      const match = stdout.match(/Mock Symphony Wave 4 server: (http:\/\/[^\s]+)/);
      if (!match) return;
      clearTimeout(timeout);
      resolve(match[1]);
    });

    child?.on("exit", (code) => {
      clearTimeout(timeout);
      reject(new Error(`mock server exited with code ${code}. stderr: ${stderr}`));
    });
  });
}
```

- [ ] **Step 2: Run mock test to verify it fails**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/wave4-mock-server.test.ts
```

Expected: FAIL because `scripts/wave4-mock-server.mjs` does not exist.

- [ ] **Step 3: Create Wave 4 mock server**

Create `apps/symphony/pi-extension/scripts/wave4-mock-server.mjs` with this content.

```js
#!/usr/bin/env node
import { createServer } from "node:http";

const options = parseArgs(process.argv.slice(2));
if (options.help) {
  printHelp();
  process.exit(0);
}

const state = createInitialState();
const responses = [];

const server = createServer((req, res) => {
  let body = "";
  req.on("data", (chunk) => {
    body += String(chunk);
  });
  req.on("end", () => {
    handleRequest(req, res, body);
  });
});

server.listen(options.port, options.host, () => {
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("Expected TCP server address");
  const baseUrl = `http://${options.host}:${address.port}`;
  console.log(`Mock Symphony Wave 4 server: ${baseUrl}`);
  console.log("");
  console.log("Attach from Pi:");
  console.log(`/symphony:attach ${baseUrl}`);
  console.log("/symphony:console");
  console.log("");
  console.log("Seeded Wave 4 state: shared context, diagnostics, events, escalation esc-1");
});

process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);

function handleRequest(req, res, body) {
  setJson(res);
  const url = new URL(req.url ?? "/", `http://${options.host}`);

  if (req.method === "GET" && url.pathname === "/api/v1/state") {
    state.shared_context = summarizeContext(state.context_entries);
    res.end(JSON.stringify(state));
    return;
  }

  if (req.method === "GET" && url.pathname === "/api/v1/context") {
    const scope = url.searchParams.get("scope");
    const entries = filterContextEntries(scope);
    res.end(JSON.stringify({ entries, summary: summarizeContext(state.context_entries) }));
    return;
  }

  if (req.method === "POST" && url.pathname === "/api/v1/context") {
    const payload = parseJsonBody(body);
    if (!payload || typeof payload.author_issue !== "string" || typeof payload.scope !== "string" || typeof payload.content !== "string") {
      res.statusCode = 400;
      res.end(JSON.stringify({ error: { code: "invalid_context", message: "author_issue, scope, and content are required", status: 400 } }));
      return;
    }
    const entry = {
      id: `ctx-${state.context_entries.length + 1}`,
      author_issue: payload.author_issue,
      scope: parseScope(payload.scope),
      content: payload.content,
      created_at: new Date().toISOString(),
      ttl_ms: typeof payload.ttl_ms === "number" ? payload.ttl_ms : 3600000,
    };
    state.context_entries.push(entry);
    state.shared_context = summarizeContext(state.context_entries);
    res.statusCode = 201;
    res.end(JSON.stringify({ id: entry.id, created_at: entry.created_at }));
    return;
  }

  if (req.method === "DELETE" && url.pathname === "/api/v1/context") {
    const scope = url.searchParams.get("scope");
    const before = state.context_entries.length;
    if (!scope) {
      state.context_entries = [];
    } else {
      state.context_entries = state.context_entries.filter((entry) => scopeKey(entry.scope) !== scope);
    }
    state.shared_context = summarizeContext(state.context_entries);
    res.end(JSON.stringify({ deleted: before - state.context_entries.length }));
    return;
  }

  const contextEntryMatch = url.pathname.match(/^\/api\/v1\/context\/([^/]+)$/);
  if (req.method === "DELETE" && contextEntryMatch) {
    const entryId = decodeURIComponent(contextEntryMatch[1]);
    const before = state.context_entries.length;
    state.context_entries = state.context_entries.filter((entry) => entry.id !== entryId);
    const deleted = before - state.context_entries.length;
    if (deleted === 0) {
      res.statusCode = 404;
      res.end(JSON.stringify({ error: { code: "context_not_found", message: `shared context entry '${entryId}' was not found`, status: 404 } }));
      return;
    }
    state.shared_context = summarizeContext(state.context_entries);
    res.end(JSON.stringify({ deleted }));
    return;
  }

  if (req.method === "GET" && url.pathname === "/api/v1/escalations") {
    res.end(JSON.stringify({ pending: state.pending_escalations }));
    return;
  }

  if (req.method === "POST" && url.pathname === "/api/v1/refresh") {
    state.polling.poll_count += 1;
    state.polling.last_poll_at = new Date().toISOString();
    res.statusCode = 202;
    res.end(JSON.stringify({ queued: true, coalesced: false, pending_requests: 1 }));
    return;
  }

  if (req.method === "POST" && url.pathname === "/api/v1/steer") {
    const payload = parseJsonBody(body);
    const instruction = typeof payload?.instruction === "string" ? payload.instruction : "";
    state.supervisor.steers_issued += 1;
    res.end(JSON.stringify({ ok: true, issue_id: "issue-123", issue_identifier: "SIM-123", delivered: true, instruction_preview: instruction.slice(0, 120) }));
    return;
  }

  const escalationMatch = url.pathname.match(/^\/api\/v1\/escalations\/([^/]+)\/respond$/);
  if (req.method === "POST" && escalationMatch) {
    const requestId = decodeURIComponent(escalationMatch[1]);
    const escalation = state.pending_escalations.find((entry) => entry.request_id === requestId);
    if (!escalation) {
      res.statusCode = 404;
      res.end(JSON.stringify({ error: "escalation_not_found" }));
      return;
    }
    responses.push({ request_id: requestId, body: parseJsonBody(body), received_at: new Date().toISOString() });
    state.pending_escalations = state.pending_escalations.filter((entry) => entry.request_id !== requestId);
    state.supervisor.escalations_created = state.pending_escalations.length;
    res.end(JSON.stringify({ ok: true }));
    return;
  }

  if (req.method === "GET" && url.pathname === "/debug/responses") {
    res.end(JSON.stringify({ responses }));
    return;
  }

  res.statusCode = 404;
  res.end(JSON.stringify({ error: "not_found" }));
}

function createInitialState() {
  const context_entries = [
    { id: "ctx-1", author_issue: "SIM-123", scope: { type: "project" }, content: "Decision: use existing auth module", created_at: "2026-05-17T12:00:00Z", ttl_ms: 3600000 },
    { id: "ctx-2", author_issue: "SIM-456", scope: { type: "label", value: "backend" }, content: "Backend worker owns API validation", created_at: "2026-05-17T12:02:00Z", ttl_ms: 3600000 },
  ];
  return {
    poll_interval_ms: 30000,
    max_concurrent_agents: 2,
    tracker_project_url: "https://linear.app/kata-sh/project/symphony",
    running: {
      "issue-123": { issue_id: "issue-123", issue_identifier: "SIM-123", issue_title: "Running worker", attempt: 1, workspace_path: "/tmp/symphony/issue-123", started_at: "2026-05-14T12:00:00Z", status: "running", tracker_state: "In Progress", worker_host: "local" },
    },
    running_sessions: { "issue-123": { turn_count: 3, last_activity_at: "2026-05-14T12:04:00Z", total_tokens: 1200, last_event: "tool_call_completed", last_event_message: "running cargo test", session_id: "session-123" } },
    running_session_info: { "issue-123": { turn_count: 3, max_turns: 20, last_activity_ms: Date.parse("2026-05-14T12:04:00Z"), session_tokens: { input_tokens: 800, output_tokens: 400, total_tokens: 1200 }, last_error: null } },
    claimed: [],
    retry_queue: [{ issue_id: "issue-retry", identifier: "SIM-200", attempt: 3, due_in_ms: 90000, error: "rate limit", worker_host: "host-b", workspace_path: "/tmp/retry" }],
    blocked: [{ issue_id: "issue-blocked", identifier: "SIM-300", title: "Blocked work", state: "Todo", blocker_identifiers: ["SIM-100", "SIM-101"] }],
    completed: [{ issue_id: "issue-done", identifier: "SIM-400", title: "Done work", completed_at: "2026-05-14T13:00:00Z" }],
    pending_escalations: [{ request_id: "esc-1", issue_id: "issue-123", issue_identifier: "SIM-123", method: "approval", preview: "Approve cargo test?", created_at: "2026-05-14T12:06:00Z", timeout_ms: 600000 }],
    context_entries,
    shared_context: summarizeContext(context_entries),
    supervisor: { active: true, steers_issued: 1, conflicts_detected: 1, patterns_detected: 1, escalations_created: 1 },
    codex_totals: { input_tokens: 800, output_tokens: 400, total_tokens: 1200, event_count: 2, seconds_running: 60 },
    codex_rate_limits: { requests: { remaining: 80, limit: 100, reset_seconds: 120 } },
    polling: { checking: false, next_poll_in_ms: 1000, poll_interval_ms: 30000, poll_count: 1, last_poll_at: "2026-05-14T12:05:00Z" },
  };
}

function filterContextEntries(scope) {
  if (!scope) return state.context_entries;
  return state.context_entries.filter((entry) => scopeKey(entry.scope) === scope);
}

function summarizeContext(entries) {
  const entries_by_scope = {};
  for (const entry of entries) entries_by_scope[scopeKey(entry.scope)] = (entries_by_scope[scopeKey(entry.scope)] ?? 0) + 1;
  const created = entries.map((entry) => entry.created_at).sort();
  return { total_entries: entries.length, entries_by_scope, oldest_entry_at: created[0] ?? null, newest_entry_at: created.at(-1) ?? null };
}

function parseScope(scope) {
  if (scope === "project") return { type: "project" };
  const [type, value] = scope.split(":");
  return { type, value };
}

function scopeKey(scope) {
  return scope.type === "project" ? "project" : `${scope.type}:${scope.value}`;
}

function parseArgs(args) {
  const parsed = { host: "127.0.0.1", port: 8787, help: false };
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--") continue;
    if (arg === "--help" || arg === "-h") {
      parsed.help = true;
      continue;
    }
    if (arg === "--host") {
      parsed.host = readValue(args, index, arg);
      index += 1;
      continue;
    }
    if (arg === "--port") {
      const rawPort = readValue(args, index, arg);
      const port = Number(rawPort);
      if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error(`Invalid --port value: ${rawPort}`);
      parsed.port = port;
      index += 1;
      continue;
    }
    throw new Error(`Unknown argument: ${arg}`);
  }
  return parsed;
}

function readValue(args, index, flag) {
  const value = args[index + 1];
  if (!value) throw new Error(`${flag} requires a value`);
  return value;
}

function parseJsonBody(body) {
  if (!body.trim()) return null;
  try {
    return JSON.parse(body);
  } catch {
    return { raw: body };
  }
}

function setJson(res) {
  res.setHeader("content-type", "application/json");
}

function shutdown() {
  server.close(() => process.exit(0));
}

function printHelp() {
  console.log(`Usage: node apps/symphony/pi-extension/scripts/wave4-mock-server.mjs [--host 127.0.0.1] [--port 8787]\n\nStarts a local mock Symphony HTTP API seeded with Wave 4 dashboard state.\n\nEndpoints:\n  GET    /api/v1/state\n  GET    /api/v1/context?scope=<scope>\n  POST   /api/v1/context\n  DELETE /api/v1/context?scope=<scope>\n  DELETE /api/v1/context/<id>\n  GET    /api/v1/escalations\n  POST   /api/v1/escalations/esc-1/respond\n  POST   /api/v1/refresh\n  POST   /api/v1/steer\n\nAttach from Pi with /symphony:attach http://127.0.0.1:<port>.`);
}
```

- [ ] **Step 4: Add package script**

In `apps/symphony/pi-extension/package.json`, add this script after `mock:wave3`.

```json
"mock:wave4": "node scripts/wave4-mock-server.mjs"
```

Keep valid JSON by adding a comma to the preceding script entry.

- [ ] **Step 5: Run mock server tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension exec vitest run src/wave4-mock-server.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/symphony/pi-extension/scripts/wave4-mock-server.mjs apps/symphony/pi-extension/src/wave4-mock-server.test.ts apps/symphony/pi-extension/package.json
git commit -m "test(pi-symphony): add wave 4 mock server"
```

---

### Task 8: Document Wave 4 and run full verification

**Files:**
- Modify: `apps/symphony/pi-extension/README.md`
- Modify: `docs/superpowers/specs/_archive/2026-05-14-pi-symphony-extension-design.md`

- [ ] **Step 1: Update README Wave 4 usage**

In `apps/symphony/pi-extension/README.md`, add this section after the existing console controls section.

```md
## Wave 4 shared context and diagnostics

Run the Wave 4 mock server from the repository root:

```bash
pnpm --dir apps/symphony/pi-extension run mock:wave4 -- --port 0
```

Attach from Pi using the URL printed by the mock server:

```text
/symphony:attach http://127.0.0.1:<port>
/symphony:console
```

Console keys through Wave 4:

- `↑` / `↓`: select issues, escalations, and shared context rows.
- `r`: request a Symphony refresh.
- `s`: steer the selected running worker.
- `e`: respond to the selected escalation.
- `c`: create shared context. Input format: `SIM-123 | project | Decision: reuse auth | 60000`.
- `x`: delete the selected shared context entry.
- `f`: filter shared context by `project`, `milestone:<id>`, or `label:<name>`. Submit a blank value to clear.
- `v`: filter the event stream with tokens such as `issue=SIM-123 type=worker severity=warn`. Submit a blank value to clear.
- `d`: toggle selected item details.
- `q` or Escape: close the console.

Diagnostics shown in the console:

- Polling state, next poll, poll interval, poll count, and last poll timestamp.
- Codex input, output, total tokens, Codex event count, and runtime duration.
- Codex rate limit buckets when Symphony reports them.
- Supervisor steers, conflicts, patterns, and escalations.
- Event stream totals, by-kind counts, by-severity counts, active filters, and last stream error.
```

- [ ] **Step 2: Mark Wave 4 complete in the spec**

In `docs/superpowers/specs/_archive/2026-05-14-pi-symphony-extension-design.md`, change this heading:

```md
### Wave 4: S05-S06 - shared context and diagnostics
```

To:

```md
### Wave 4: S05-S06 - shared context and diagnostics ✅
```

- [ ] **Step 3: Run package tests**

Run:

```bash
pnpm --dir apps/symphony/pi-extension run test
```

Expected: PASS.

- [ ] **Step 4: Run typecheck**

Run:

```bash
pnpm --dir apps/symphony/pi-extension run typecheck
```

Expected: PASS.

- [ ] **Step 5: Run lint**

Run:

```bash
pnpm --dir apps/symphony/pi-extension run lint
```

Expected: PASS.

- [ ] **Step 6: Run affected validation**

Run from repository root:

```bash
pnpm run validate:affected
```

Expected: PASS.

- [ ] **Step 7: Commit docs and verification updates**

```bash
git add apps/symphony/pi-extension/README.md docs/superpowers/specs/_archive/2026-05-14-pi-symphony-extension-design.md
git commit -m "docs(pi-symphony): document wave 4 console parity"
```

---

## Self-Review

Spec coverage:

- Shared context entries and summary render in Task 5.
- Shared context create and delete operations are implemented in Tasks 1, 3, and 5.
- Scope filtering is implemented in Tasks 2, 3, and 5.
- Rate limits, polling diagnostics, token totals, event stream counters, and event filters are implemented in Tasks 2, 4, and 5.
- Help text, key hints, empty states, and layout polish are implemented in Tasks 5 and 6.
- Mock HTTP server coverage is implemented in Task 7.
- Documentation and verification are covered in Task 8.

Placeholder scan:

- The plan contains concrete file paths, test code, implementation snippets, commands, expected results, and commit commands.
- No task relies on undefined future work.

Type consistency:

- Shared context response types use Symphony's serialized `ContextScope` shape: `{ type: "project" }`, `{ type: "milestone", value: string }`, and `{ type: "label", value: string }`.
- Console state uses `contextScopeFilter` and `eventFilters` consistently across `state.ts`, `runtime.ts`, `console.ts`, and tests.
- Runtime methods match console option names: `refreshSharedContext`, `createSharedContext`, `deleteSharedContextEntry`, and `clearSharedContextScope`.
