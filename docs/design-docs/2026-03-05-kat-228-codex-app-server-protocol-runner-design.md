# KAT-228 Codex App-Server Client Protocol Runner Design

## Context

- Ticket: `KAT-228`
- Branch: `feature/kat-228-plan-build-codex-app-server-client-protocol-runner`
- Goal: Implement `SPEC.md` Section `10` coding-agent integration contract for the app-server protocol runner.
- Project/Epic context reviewed:
  - Linear project: `Symphony Service v1 (Spec Execution)`
  - Milestone: `M2 Parallel Subsystems`
  - Upstream chain: `KAT-255` -> `KAT-221` -> (`KAT-223`, `KAT-224`) -> `KAT-228`
  - Parallel lane contract comment on `KAT-228` (2026-03-05): this ticket owns codex protocol runner only.
- Dependency status verified in start sequence:
  - `KAT-223` (`Done`)
  - `KAT-224` (`Done`)

## References Reviewed

- Linear issue: `KAT-228` (description, acceptance criteria, relations)
- Linear issue comment contract: `1fca6792-a1ec-4ff7-8f66-669dfac5c4e2`
- Referenced parent/foundation issues:
  - `KAT-221` (core domain/contracts scaffold)
  - `KAT-255` (TS scaffold/toolchain baseline)
- Linear docs:
  - `Project Spec` (`c02d5eb5a8d1`)
  - `Symphony v1 Execution Plan (Dependency DAG)` (`5b235d1e8099`)
- Repository specs/docs:
  - `SPEC.md` Section `10` (`10.1`-`10.7`), Section `16.5`, Section `17.5`, Section `18.1`
  - `ARCHITECTURE.md`
  - `WORKFLOW.md` (`codex.command`, timeout defaults)
- Existing code surface:
  - `src/execution/contracts.ts`
  - `src/domain/models.ts`
  - `src/config/types.ts`, `src/config/defaults.ts`, `src/config/contracts.ts`
  - `src/orchestrator/contracts.ts`
- Attached issue docs/design mocks:
  - No direct attachments or design mocks are attached to `KAT-228`.

## Requirements Summary

1. Launch codex process as `bash -lc <codex.command>` with workspace path as `cwd`.
2. Startup handshake ordering must be:
   - `initialize` request
   - `initialized` notification
   - `thread/start` request
   - `turn/start` request
3. Parse line-delimited JSON protocol from stdout only.
4. Buffer partial stdout lines until newline before parsing.
5. Keep stderr separate and diagnostic-only.
6. Enforce timeout behavior for:
   - `read_timeout_ms`
   - `turn_timeout_ms`
   - `stall_timeout_ms`
7. Reuse thread for continuation turns in a single run.
8. Populate session metadata needed by orchestrator:
   - thread/turn/session IDs
   - codex PID
   - usage/token totals
   - last event/timestamp/message
9. Return typed runtime outcomes suitable for retry/reconciliation decisions.
10. Stay within ticket boundary: no tracker adapter or workspace manager policy changes.

## Assumptions and Clarifications

1. `AgentRunner.runAttempt(issue, attempt)` is the integration seam for this ticket; result remains:
   - `{ attempt: RunAttempt; session: LiveSession | null }`
2. This ticket introduces protocol-runner implementation modules under execution layer but avoids widening shared domain contracts unless additive and strictly required.
3. Issue-level state refresh and dispatch/retry policy decisions remain orchestrator concerns (`KAT-229`, `KAT-232`).
4. `stall_timeout_ms <= 0` disables runner-side stall timeout checks (spec behavior).
5. Current implementation posture for input-required is fail-fast (`turn_input_required`) to avoid indefinite stall.

## Options Considered

1. Single monolithic runner loop (spawn + parse + handshake + turn + metadata in one function)
   - Pros: fastest initial implementation, minimal file count.
   - Cons: hard to test in isolation, fragile timeout/error handling, poor extensibility for protocol variants.

2. Layered runner with transport + protocol + session reducer (recommended)
   - Pros: testable boundaries, deterministic handshake/state transitions, clear timeout ownership, easier compatibility shims for payload variants.
   - Cons: more initial module scaffolding.

3. Event-sourced finite-state machine (FSM) with explicit transition table
   - Pros: strongest formal correctness, very clear event traces.
   - Cons: overkill for current codebase maturity; higher implementation overhead for this milestone.

## Selected Approach

Use **Option 2**: layered runner architecture.

This best fits the current scaffold and allows tight tests for handshake order, partial-line framing, stderr isolation, timeout mapping, and session metadata accumulation without coupling to orchestrator internals.

## Proposed Design

### 1. Module Layout

Add execution modules under `src/execution/agent-runner/`:

1. `index.ts`
   - exports `createAgentRunner(deps)`
2. `runner.ts`
   - implements `AgentRunner.runAttempt(issue, attempt)` orchestration
3. `transport.ts`
   - process spawn/IO lifecycle (`bash -lc`, cwd, stdout/stderr handlers)
4. `line-buffer.ts`
   - incremental line decoder for partial stdout chunks
5. `protocol-client.ts`
   - request/response + notification plumbing and handshake ordering
6. `session-reducer.ts`
   - maps protocol events/messages into `LiveSession` updates + token/rate-limit aggregation
7. `errors.ts`
   - typed error taxonomy and normalization mapping

### 2. `AgentRunner` Flow

`runAttempt(issue, attempt)` sequence:

1. Resolve effective codex config snapshot and workspace `cwd` from existing contracts.
2. Spawn process with:
   - executable: `bash`
   - args: `-lc`, `<codex.command>`
   - cwd: workspace path
   - stdio: separate stdout/stderr pipes
3. Initialize protocol client on stdout lines (stderr side channel for diagnostics only).
4. Execute startup handshake in strict order (`initialize` -> `initialized` -> `thread/start` -> `turn/start`).
5. Start turn stream loop until terminal turn event (`turn/completed`, `turn/failed`, `turn/cancelled`) or failure timeout.
6. Update/return:
   - `RunAttempt` with final status + normalized error (if any)
   - `LiveSession` with latest IDs/tokens/event metadata (or `null` if startup failed before session creation)

### 3. Handshake Contract

`protocol-client` enforces:

1. `initialize` request with `clientInfo` + `capabilities`.
2. Wait for `initialize` response under `read_timeout_ms`.
3. Send `initialized` notification.
4. Send `thread/start` request with approval/sandbox/cwd payload.
5. Parse `thread_id` from `result.thread.id` (including compatible equivalent payload shapes).
6. Send `turn/start` request with `threadId`, prompt input, cwd/title, approval/sandbox policy.
7. Parse `turn_id` from `result.turn.id` and derive `session_id = "<thread_id>-<turn_id>"`.

### 4. Stream Parsing and IO Separation

1. Stdout only:
   - append chunk -> line buffer
   - parse complete lines as JSON
   - keep incomplete fragment buffered until newline
2. Stderr only:
   - never parsed as protocol JSON
   - forwarded as diagnostic events/log records
3. Malformed stdout line behavior:
   - emit structured `malformed` event
   - continue stream unless policy marks fatal (default: non-fatal malformed notification)

### 5. Timeout Model

1. `read_timeout_ms`
   - used for startup request/response waits and any synchronous protocol response waits.
   - maps to `response_timeout`.
2. `turn_timeout_ms`
   - wall-clock timeout across one `turn/start` stream.
   - maps to `turn_timeout`.
3. `stall_timeout_ms`
   - inactivity timeout based on elapsed time since last meaningful protocol event in active turn stream.
   - disabled when `<= 0`.
   - maps to `turn_timeout` or `response_timeout` category with explicit stall context (implementation detail documented in error payload).

### 6. Error Taxonomy and Mapping

Normalized failure kinds (from Section `10.6`):

- `codex_not_found`
- `invalid_workspace_cwd`
- `response_timeout`
- `turn_timeout`
- `port_exit`
- `response_error`
- `turn_failed`
- `turn_cancelled`
- `turn_input_required`

Mapping rules:

1. Spawn `ENOENT` -> `codex_not_found`
2. Cwd/path launch failure -> `invalid_workspace_cwd`
3. Request wait timeout -> `response_timeout`
4. Turn stream hard timeout -> `turn_timeout`
5. Unexpected process exit while active -> `port_exit`
6. Protocol error response payload -> `response_error`
7. Terminal turn failure/cancelled events -> `turn_failed` / `turn_cancelled`
8. Input-required signal -> `turn_input_required`

### 7. Session Metadata and Runtime Event Surface

`session-reducer` owns canonical `LiveSession` updates:

1. startup:
   - set pid (if available)
   - set thread/turn/session IDs
2. per message/event:
   - update `last_codex_event`, `last_codex_timestamp`, `last_codex_message`
3. usage/rate-limit extraction:
   - support nested compatible payload variants
   - keep cumulative totals and last-reported snapshots
4. completion/failure:
   - preserve final metadata for orchestrator reconciliation/retry logging

### 8. Continuation Turns

Within a single worker run:

1. Preserve live process and `thread_id`.
2. For subsequent turns, send `turn/start` with same `threadId`.
3. Increment `turn_count` and replace `turn_id`/`session_id` per turn.
4. Stop process only when worker run exits.

## Testing Strategy

## Unit Tests

1. `line-buffer` buffers partial chunks and emits complete lines only on newline.
2. stdout JSON parse handles split chunks and multiple lines per chunk.
3. stderr lines are ignored by protocol parser.
4. handshake order assertion with mocked transport writes.
5. ID extraction supports expected payload shapes and guarded fallbacks.
6. timeout utilities enforce `read_timeout_ms` and `turn_timeout_ms`.
7. stall timeout behavior for enabled/disabled settings.
8. error normalization maps raw failures to typed categories.

## Integration Tests (Execution Layer)

1. Spawn contract uses `bash -lc <command>` with workspace cwd.
2. Full startup transcript success path sets `thread_id`, `turn_id`, `session_id`.
3. `turn/completed` returns success attempt + non-null session metadata.
4. `turn/failed`, `turn/cancelled`, subprocess exit return typed failed attempts.
5. malformed stdout JSON line does not crash parser (unless fatal policy chosen).
6. usage/rate-limit updates flow into `LiveSession` token fields.

## Conformance Traceability (Section 17.5)

- Launch contract (`bash -lc`, cwd)
- Handshake ordering
- read/turn timeout enforcement
- partial-line buffering
- stdout/stderr separation
- session ID extraction
- usage/rate-limit extraction
- non-stalling behavior for unsupported/input-required paths

## Verification Commands (when implementation starts)

1. `pnpm run lint`
2. `pnpm run typecheck`
3. `pnpm test`
4. `make check`

## Scope Boundaries

### In Scope

- App-server protocol runner internals
- Startup handshake, stream parsing, timeout/error mapping
- Session metadata emission for orchestrator consumption
- Tests for protocol behavior and parsing correctness

### Out of Scope

- Tracker querying/normalization (`KAT-226`)
- Workspace sanitization/hook policy (`KAT-227`)
- Startup/tick dispatch preflight policy (`KAT-225`)
- Orchestrator retry scheduling/reconciliation policy (`KAT-229`, `KAT-232`)

## Risks and Mitigations

1. Risk: protocol payload drift across codex app-server versions.
   - Mitigation: compatibility extractors for logical fields + tests for variant payloads.
2. Risk: parser deadlocks on partial/broken streams.
   - Mitigation: bounded line size, fragment buffering tests, hard timeout guards.
3. Risk: timeout overlap ambiguity (`turn_timeout` vs `stall_timeout`).
   - Mitigation: centralized timer policy with explicit precedence and structured reason fields.
4. Risk: noisy stderr causes false failures.
   - Mitigation: strict stderr diagnostic channel separation.

## Handoff

This design defines the codex app-server protocol runner architecture for `KAT-228`, aligned to `SPEC.md` Section `10`, scoped for parallel execution with `KAT-225`/`KAT-226`/`KAT-227`, and ready for implementation planning.
