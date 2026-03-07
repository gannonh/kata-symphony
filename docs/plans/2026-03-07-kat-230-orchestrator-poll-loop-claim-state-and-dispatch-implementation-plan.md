# KAT-230 Orchestrator Poll Loop, Claim State, and Dispatch Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the no-op orchestrator with a concrete poll-loop implementation that owns claim state, sorts and gates candidates deterministically, dispatches worker attempts safely, and exposes stable seams for retries, reconciliation, and observability.

**Architecture:** Build the orchestrator in thin vertical slices. First tighten the execution and orchestrator contracts so live codex events can reach the orchestrator and the bootstrap can pass the correct dependencies. Then add pure orchestration helpers for runtime state, candidate selection, and state transitions before composing them into a tick runner and a concrete start/stop scheduler. Keep state mutation serialized inside the orchestrator, follow TDD for every slice, and commit after each passing slice.

**Tech Stack:** TypeScript, Node.js 22, pnpm, Vitest, ESLint, repo harness scripts, `make check`

---

## Implementation Notes

- Use `@superpowers:test-driven-development` for every task below.
- Use `@symphony-harness-evidence` before final completion because this change touches `src/orchestrator/**`.
- Use `@superpowers:verification-before-completion` before claiming success.
- Keep the orchestrator as the only mutator of `claimed`, `running`, retry bookkeeping, and aggregate codex counters.
- Do not implement full retry timers or full reconciliation internals here; only define and consume the seams needed by `KAT-231` and `KAT-232`.

### Task 1: Expose Worker-Attempt Event Callbacks to the Orchestrator

**Files:**
- Modify: `src/execution/worker-attempt/contracts.ts`
- Modify: `src/execution/worker-attempt/run-worker-attempt.ts`
- Modify: `src/execution/contracts.ts`
- Modify: `src/orchestrator/contracts.ts`
- Modify: `src/bootstrap/service.ts`
- Test: `tests/execution/worker-attempt/worker-attempt-contracts.test.ts`
- Test: `tests/contracts/layer-contracts.test.ts`
- Test: `tests/bootstrap/service-worker-attempt-wiring.test.ts`
- Test: `tests/bootstrap/service-wiring.test.ts`

**Step 1: Write the failing tests**

Add coverage that forces the new per-run callback contract:

```ts
type WorkerAttemptEventHandler = (event: unknown) => void

const runner: WorkerAttemptRunner = {
  async run(issue, attempt, options) {
    options?.onCodexEvent?.({ issue_id: issue.id, event: 'turn_completed' })
    throw new Error('not implemented')
  },
}

expect(typeof runner.run).toBe('function')
```

Update the bootstrap wiring test to stop expecting direct logger ownership of worker events and instead assert that the worker-attempt runner accepts a callback from its caller.

**Step 2: Run tests to verify they fail**

Run:

```bash
pnpm vitest run \
  tests/execution/worker-attempt/worker-attempt-contracts.test.ts \
  tests/contracts/layer-contracts.test.ts \
  tests/bootstrap/service-worker-attempt-wiring.test.ts \
  tests/bootstrap/service-wiring.test.ts
```

Expected: FAIL because `WorkerAttemptRunner.run()` still takes only `(issue, attempt)` and bootstrap still hard-wires `onCodexEvent` to the logger.

**Step 3: Write the minimal implementation**

Change the worker-attempt contract to accept per-run options:

```ts
export interface WorkerAttemptRunOptions {
  onCodexEvent?: (event: unknown) => void
}

export interface WorkerAttemptRunner {
  run(
    issue: Issue,
    attempt: number | null,
    options?: WorkerAttemptRunOptions,
  ): Promise<WorkerAttemptResult>
}
```

In `run-worker-attempt.ts`, prefer `options?.onCodexEvent` over the constructor-scoped callback. In `src/bootstrap/service.ts`, stop using the logger as the permanent sink for worker-attempt codex events. In `src/orchestrator/contracts.ts`, add `workerAttemptRunner` to `OrchestratorDeps` so the real orchestrator can own the callback wiring.

**Step 4: Run tests to verify they pass**

Run:

```bash
pnpm vitest run \
  tests/execution/worker-attempt/worker-attempt-contracts.test.ts \
  tests/contracts/layer-contracts.test.ts \
  tests/bootstrap/service-worker-attempt-wiring.test.ts \
  tests/bootstrap/service-wiring.test.ts
```

Expected: PASS

**Step 5: Commit**

```bash
git add \
  src/execution/worker-attempt/contracts.ts \
  src/execution/worker-attempt/run-worker-attempt.ts \
  src/execution/contracts.ts \
  src/orchestrator/contracts.ts \
  src/bootstrap/service.ts \
  tests/execution/worker-attempt/worker-attempt-contracts.test.ts \
  tests/contracts/layer-contracts.test.ts \
  tests/bootstrap/service-worker-attempt-wiring.test.ts \
  tests/bootstrap/service-wiring.test.ts
git commit -m "feat(orchestrator): expose worker attempt events to orchestrator"
```

### Task 2: Add Concrete Orchestrator Runtime Contracts

**Files:**
- Create: `src/orchestrator/runtime/contracts.ts`
- Create: `src/orchestrator/runtime/index.ts`
- Modify: `src/orchestrator/contracts.ts`
- Modify: `tests/contracts/runtime-modules.test.ts`
- Create: `tests/orchestrator/runtime-contracts.test.ts`

**Step 1: Write the failing tests**

Add a dedicated runtime contract test:

```ts
const runningEntry: RunningEntry = {
  issue: issueFixture,
  identifier: issueFixture.identifier,
  workerPromise: Promise.resolve(),
  retry_attempt: null,
  started_at: '2026-03-07T00:00:00Z',
  session_id: null,
  codex_app_server_pid: null,
  last_codex_event: null,
  last_codex_timestamp: null,
  last_codex_message: null,
  codex_input_tokens: 0,
  codex_output_tokens: 0,
  codex_total_tokens: 0,
  last_reported_input_tokens: 0,
  last_reported_output_tokens: 0,
  last_reported_total_tokens: 0,
}

expect(deriveClaimState('issue-1', state)).toBe('claimed_running')
```

Also extend the runtime modules test to import the new orchestrator runtime barrel.

**Step 2: Run tests to verify they fail**

Run:

```bash
pnpm vitest run \
  tests/orchestrator/runtime-contracts.test.ts \
  tests/contracts/runtime-modules.test.ts
```

Expected: FAIL because `src/orchestrator/runtime/*` does not exist yet.

**Step 3: Write the minimal implementation**

Create `src/orchestrator/runtime/contracts.ts` with the concrete runtime types:

```ts
export interface RunningEntry {
  issue: Issue
  identifier: string
  workerPromise: Promise<void> | null
  retry_attempt: number | null
  started_at: string
  session_id: string | null
  codex_app_server_pid: string | null
  last_codex_event: string | null
  last_codex_timestamp: string | null
  last_codex_message: string | null
  codex_input_tokens: number
  codex_output_tokens: number
  codex_total_tokens: number
  last_reported_input_tokens: number
  last_reported_output_tokens: number
  last_reported_total_tokens: number
}

export interface OrchestratorState {
  poll_interval_ms: number
  max_concurrent_agents: number
  running: Map<string, RunningEntry>
  claimed: Set<string>
  retry_attempts: Map<string, RetryEntry>
  completed: Set<string>
  codex_totals: CodexTotals
  codex_rate_limits: unknown
}
```

Export a helper like `deriveClaimState(issueId, state)` plus any small runtime helper types needed later. Re-export them from `src/orchestrator/contracts.ts`.

**Step 4: Run tests to verify they pass**

Run:

```bash
pnpm vitest run \
  tests/orchestrator/runtime-contracts.test.ts \
  tests/contracts/runtime-modules.test.ts
```

Expected: PASS

**Step 5: Commit**

```bash
git add \
  src/orchestrator/runtime/contracts.ts \
  src/orchestrator/runtime/index.ts \
  src/orchestrator/contracts.ts \
  tests/orchestrator/runtime-contracts.test.ts \
  tests/contracts/runtime-modules.test.ts
git commit -m "feat(orchestrator): add concrete runtime contracts"
```

### Task 3: Implement Candidate Sorting and Eligibility Helpers

**Files:**
- Create: `src/orchestrator/runtime/dispatch-selection.ts`
- Modify: `src/orchestrator/runtime/index.ts`
- Create: `tests/orchestrator/dispatch-selection.test.ts`

**Step 1: Write the failing tests**

Cover the pure rules from `SPEC.md` Section `8.2`:

```ts
expect(sortCandidatesForDispatch([late, early, nullPriority])).toEqual([
  early,
  late,
  nullPriority,
])

expect(
  shouldDispatch(todoWithOpenBlocker, state, {
    terminalStates: ['done'],
    perStateLimits: {},
  }),
).toBe(false)
```

Add cases for:
- missing structural fields
- already claimed
- already running
- global slot exhaustion
- per-state slot exhaustion
- `Todo` with terminal blockers
- tie-break on `identifier`

**Step 2: Run tests to verify they fail**

Run:

```bash
pnpm vitest run tests/orchestrator/dispatch-selection.test.ts
```

Expected: FAIL because the selection helpers do not exist yet.

**Step 3: Write the minimal implementation**

Implement pure helpers:

```ts
export function sortCandidatesForDispatch(issues: Issue[]): Issue[] {
  return [...issues].sort(compareIssuesForDispatch)
}

export function shouldDispatch(
  issue: Issue,
  state: OrchestratorState,
  options: DispatchEligibilityOptions,
): boolean {
  return (
    isCandidateStructurallyEligible(issue) &&
    !state.running.has(issue.id) &&
    !state.claimed.has(issue.id) &&
    hasAvailableGlobalSlot(state) &&
    hasAvailableStateSlot(issue.state, state, options.perStateLimits) &&
    passesTodoBlockerGate(issue, options.terminalStates)
  )
}
```

Keep state normalization logic local and deterministic. Do not embed any timer or reconciliation concerns here.

**Step 4: Run tests to verify they pass**

Run:

```bash
pnpm vitest run tests/orchestrator/dispatch-selection.test.ts
```

Expected: PASS

**Step 5: Commit**

```bash
git add \
  src/orchestrator/runtime/dispatch-selection.ts \
  src/orchestrator/runtime/index.ts \
  tests/orchestrator/dispatch-selection.test.ts
git commit -m "feat(orchestrator): add dispatch selection helpers"
```

### Task 4: Implement Runtime State Transitions and Codex Aggregation

**Files:**
- Create: `src/orchestrator/runtime/state-machine.ts`
- Modify: `src/orchestrator/runtime/index.ts`
- Create: `tests/orchestrator/state-machine.test.ts`

**Step 1: Write the failing tests**

Add reducer-style tests for the orchestrator-owned transitions:

```ts
const next = claimRunningIssue(state, issue, {
  workerPromise: Promise.resolve(),
  retry_attempt: null,
  started_at: '2026-03-07T00:00:00Z',
})

expect(next.claimed.has(issue.id)).toBe(true)
expect(next.running.get(issue.id)?.identifier).toBe(issue.identifier)

const updated = applyCodexUpdate(next, issue.id, {
  session: { session_id: 'thread-1-turn-1', codex_total_tokens: 9 },
  rate_limits: { requests_remaining: 10 },
})

expect(updated.codex_totals.total_tokens).toBe(9)
```

Also test:
- releasing an issue removes it from `claimed`, `running`, and `retry_attempts`
- recording completion adds bookkeeping only
- worker exit transforms state into a retry request or release request payload, not direct timer logic

**Step 2: Run tests to verify they fail**

Run:

```bash
pnpm vitest run tests/orchestrator/state-machine.test.ts
```

Expected: FAIL because the state transition helpers do not exist yet.

**Step 3: Write the minimal implementation**

Implement immutable helpers in `state-machine.ts`:

```ts
export function createInitialOrchestratorState(snapshot: ConfigSnapshot): OrchestratorState {
  return {
    poll_interval_ms: snapshot.polling.interval_ms,
    max_concurrent_agents: snapshot.agent.max_concurrent_agents,
    running: new Map(),
    claimed: new Set(),
    retry_attempts: new Map(),
    completed: new Set(),
    codex_totals: { input_tokens: 0, output_tokens: 0, total_tokens: 0, seconds_running: 0 },
    codex_rate_limits: null,
  }
}

export function releaseIssue(state: OrchestratorState, issueId: string): OrchestratorState {
  const next = cloneState(state)
  next.claimed.delete(issueId)
  next.running.delete(issueId)
  next.retry_attempts.delete(issueId)
  return next
}
```

Add helpers to:
- claim a running issue
- record codex session updates into both `RunningEntry` and aggregate totals
- compute next retry intent from a worker outcome
- release and completion bookkeeping

Keep retry scheduling as returned intent data for `KAT-231`, not actual `setTimeout` code.

**Step 4: Run tests to verify they pass**

Run:

```bash
pnpm vitest run tests/orchestrator/state-machine.test.ts
```

Expected: PASS

**Step 5: Commit**

```bash
git add \
  src/orchestrator/runtime/state-machine.ts \
  src/orchestrator/runtime/index.ts \
  tests/orchestrator/state-machine.test.ts
git commit -m "feat(orchestrator): add runtime state transitions"
```

### Task 5: Implement Tick Execution and Dispatch Handoff

**Files:**
- Create: `src/orchestrator/runtime/run-poll-tick.ts`
- Modify: `src/orchestrator/runtime/index.ts`
- Create: `tests/orchestrator/run-poll-tick.test.ts`

**Step 1: Write the failing tests**

Cover spec tick order and dispatch stop conditions:

```ts
const result = await runPollTick({
  state,
  reconcile: vi.fn(async () => reconciledState),
  validate: vi.fn(async () => ({ ok: true })),
  fetchCandidates: vi.fn(async () => [todoA, todoB]),
  dispatchIssue: vi.fn(async (issue) => stateAfterDispatch(issue)),
})

expect(reconcile).toHaveBeenCalledBefore(validate)
expect(validate).toHaveBeenCalledBefore(fetchCandidates)
expect(dispatchIssue).toHaveBeenCalledTimes(1)
```

Add cases for:
- validation failure skips fetch/dispatch
- fetch failure skips dispatch and returns unchanged state
- slot exhaustion stops iteration
- already claimed/running issues are skipped
- sort order is honored before dispatching

**Step 2: Run tests to verify they fail**

Run:

```bash
pnpm vitest run tests/orchestrator/run-poll-tick.test.ts
```

Expected: FAIL because `runPollTick` does not exist yet.

**Step 3: Write the minimal implementation**

Implement `runPollTick` as a composition layer:

```ts
export async function runPollTick(options: RunPollTickOptions): Promise<OrchestratorState> {
  let state = await options.reconcile(options.state)

  const gate = await runTickPreflightGate({
    reconcile: async () => {},
    validate: options.validate,
    logFailure: options.logFailure,
  })

  if (!gate.dispatchAllowed) {
    return state
  }

  const issues = await options.fetchCandidates().catch(() => null)
  if (!issues) {
    return state
  }

  for (const issue of sortCandidatesForDispatch(issues)) {
    if (!hasAvailableGlobalSlot(state)) break
    if (!shouldDispatch(issue, state, options.selection)) continue
    state = await options.dispatchIssue(state, issue, null)
  }

  return state
}
```

Keep reconciliation itself injected so `KAT-232` can replace the no-op or simple version later.

**Step 4: Run tests to verify they pass**

Run:

```bash
pnpm vitest run tests/orchestrator/run-poll-tick.test.ts
```

Expected: PASS

**Step 5: Commit**

```bash
git add \
  src/orchestrator/runtime/run-poll-tick.ts \
  src/orchestrator/runtime/index.ts \
  tests/orchestrator/run-poll-tick.test.ts
git commit -m "feat(orchestrator): add poll tick executor"
```

### Task 6: Build the Concrete Orchestrator Service and Bootstrap Wiring

**Files:**
- Create: `src/orchestrator/service.ts`
- Modify: `src/orchestrator/contracts.ts`
- Modify: `src/bootstrap/service.ts`
- Modify: `tests/bootstrap/service-wiring.test.ts`
- Modify: `tests/bootstrap/service-internals.test.ts`
- Modify: `tests/bootstrap/startup.test.ts`
- Create: `tests/orchestrator/service.test.ts`

**Step 1: Write the failing tests**

Add service-level tests with fake timers:

```ts
vi.useFakeTimers()

const orchestrator = createOrchestrator(deps)
await orchestrator.start()

expect(runPollTickSpy).toHaveBeenCalledTimes(1)

await vi.advanceTimersByTimeAsync(30_000)
expect(runPollTickSpy).toHaveBeenCalledTimes(2)

await orchestrator.stop()
await vi.advanceTimersByTimeAsync(30_000)
expect(runPollTickSpy).toHaveBeenCalledTimes(2)
```

Update bootstrap tests to assert:
- `createService()` builds a concrete orchestrator
- `startService()` starts orchestration successfully after startup preflight
- startup no longer reports `orchestration_enabled: false`

**Step 2: Run tests to verify they fail**

Run:

```bash
pnpm vitest run \
  tests/orchestrator/service.test.ts \
  tests/bootstrap/service-wiring.test.ts \
  tests/bootstrap/service-internals.test.ts \
  tests/bootstrap/startup.test.ts
```

Expected: FAIL because the orchestrator is still a no-op and no timer-driven service exists.

**Step 3: Write the minimal implementation**

Create a concrete orchestrator:

```ts
export function createOrchestrator(deps: OrchestratorDeps): Orchestrator {
  let state = createInitialOrchestratorState(deps.config.getSnapshot())
  let timer: NodeJS.Timeout | null = null
  let stopped = false

  const scheduleNextTick = () => {
    if (stopped) return
    timer = setTimeout(() => void tick(), state.poll_interval_ms)
  }

  const tick = async () => {
    state = await runPollTick({ ...buildTickDeps(deps), state })
    scheduleNextTick()
  }

  return {
    async start() {
      stopped = false
      await tick()
    },
    async stop() {
      stopped = true
      if (timer) clearTimeout(timer)
    },
  }
}
```

Wire `createService()` to construct `createOrchestrator(...)` with `workerAttemptRunner`. Keep dispatch implementation limited to launching worker promises, claiming issues, and routing `onCodexEvent` plus `worker_exit` back into orchestrator-owned transitions. Do not add full retry timer scheduling or full reconciliation logic here.

**Step 4: Run tests to verify they pass**

Run:

```bash
pnpm vitest run \
  tests/orchestrator/service.test.ts \
  tests/bootstrap/service-wiring.test.ts \
  tests/bootstrap/service-internals.test.ts \
  tests/bootstrap/startup.test.ts
```

Expected: PASS

**Step 5: Commit**

```bash
git add \
  src/orchestrator/service.ts \
  src/orchestrator/contracts.ts \
  src/bootstrap/service.ts \
  tests/orchestrator/service.test.ts \
  tests/bootstrap/service-wiring.test.ts \
  tests/bootstrap/service-internals.test.ts \
  tests/bootstrap/startup.test.ts
git commit -m "feat(orchestrator): add concrete poll loop service"
```

### Task 7: Update Architecture Docs and Run Full Verification

**Files:**
- Modify: `ARCHITECTURE.md`
- Modify: `docs/plans/2026-03-07-kat-230-orchestrator-poll-loop-claim-state-and-dispatch-design.md`

**Step 1: Write the failing docs/tests expectation**

List the docs deltas before editing:

```md
- `ARCHITECTURE.md` still describes the coordination layer abstractly and does not mention the concrete runtime contracts or worker-attempt event handoff.
- The KAT-230 design doc should note any implementation deltas discovered while coding.
```

**Step 2: Run targeted verification before docs edits**

Run:

```bash
pnpm vitest run tests/orchestrator tests/bootstrap
pnpm typecheck
```

Expected: PASS before docs edits. If anything fails here, fix code before touching docs.

**Step 3: Write the minimal documentation updates**

Update `ARCHITECTURE.md` to describe:

```md
3. Coordination layer
   - `src/orchestrator/contracts.ts`
   - `src/orchestrator/runtime/contracts.ts`
   - `src/orchestrator/runtime/dispatch-selection.ts`
   - `src/orchestrator/runtime/state-machine.ts`
   - `src/orchestrator/runtime/run-poll-tick.ts`
   - `src/orchestrator/service.ts`
```

Update the design doc only if implementation forced a meaningful boundary change.

**Step 4: Run full verification**

Run:

```bash
pnpm lint
pnpm typecheck
pnpm test
make check
```

Expected: PASS

**Step 5: Commit**

```bash
git add \
  ARCHITECTURE.md \
  docs/plans/2026-03-07-kat-230-orchestrator-poll-loop-claim-state-and-dispatch-design.md
git commit -m "docs: sync orchestrator architecture and design notes"
```

## Final Verification Checklist

- `WorkerAttemptRunner` accepts per-run codex event callbacks from the orchestrator.
- `OrchestratorDeps` receives `workerAttemptRunner`.
- Concrete runtime contracts exist under `src/orchestrator/runtime/`.
- Dispatch sorting and eligibility rules are covered by dedicated unit tests.
- Orchestrator state transitions are covered by dedicated unit tests.
- Tick order is covered by dedicated unit tests.
- Concrete orchestrator `start()` / `stop()` behavior is covered by fake-timer tests.
- Bootstrap/startup tests still pass with the real orchestrator wired in.
- `ARCHITECTURE.md` reflects the concrete coordination-layer modules.
- `pnpm lint`
- `pnpm typecheck`
- `pnpm test`
- `make check`
