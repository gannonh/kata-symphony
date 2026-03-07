# KAT-230 Orchestrator Poll Loop, Claim State, and Dispatch Design

## Context

- Ticket: `KAT-230`
- Branch: `feature/kat-230-plan-implement-orchestrator-poll-loop-claim-state-and`
- Goal: implement the orchestration core from `SPEC.md` Sections `7`, `8.1`-`8.3`, `16.2`, and `16.4`.
- Project: `Symphony Service v1 (Spec Execution)`
- Milestone: `M3 Orchestrator Integration`

Dependency and project context reviewed:

- Blockers verified `Done` during start sequence:
  - `KAT-225` dispatch preflight validation
  - `KAT-226` Linear tracker adapter and normalization
  - `KAT-229` worker attempt pipeline
- Downstream blocked issues reviewed:
  - `KAT-231`, `KAT-232`, `KAT-233`, `KAT-234`, `KAT-235`, `KAT-236`, `KAT-253`
- No parent epic is attached directly to `KAT-230` in Linear issue metadata.
- No issue-level attachments, docs, or design mocks are attached directly to `KAT-230`.

## References Reviewed

- Linear issue: `KAT-230`
- Linear project docs:
  - `Project Spec`
  - `Symphony v1 Execution Plan (Dependency DAG)`
- Repository source-of-truth docs:
  - `SPEC.md` Sections `7.1`-`7.4`, `8.1`-`8.3`, `16.2`, `16.4`, `17.4`, `18.1`
  - `ARCHITECTURE.md`
  - `RELIABILITY.md`
  - `SECURITY.md`
  - `WORKFLOW.md`
- Existing related designs:
  - `docs/design-docs/2026-03-05-kat-225-dispatch-preflight-validation-design.md`
  - `docs/design-docs/2026-03-05-kat-229-worker-attempt-pipeline-design.md`
- Current code seams:
  - `src/orchestrator/contracts.ts`
  - `src/bootstrap/service.ts`
  - `src/tracker/contracts.ts`
  - `src/execution/contracts.ts`
  - `src/execution/worker-attempt/*`

## Problem Statement

Current code has a concrete worker-attempt runner and preflight gate, but the orchestrator itself is
still a no-op shell. `KAT-230` needs to turn the existing subsystem seams into a real
single-authority orchestration core that:

1. owns the mutable runtime state,
2. reconciles running work before each dispatch cycle,
3. fetches and sorts eligible candidates deterministically,
4. applies global and per-state concurrency limits,
5. dispatches work without double-claiming,
6. defines the runtime contracts consumed by retry, reconciliation, cleanup, and observability
   tickets.

The architecture decision for this ticket is intentional: `KAT-230` should define the full
orchestrator state model and event/reducer seams, while downstream tickets implement specialized
event producers and policy details inside those seams.

## Goals

1. Make `KAT-230` the architectural anchor for M3 orchestration behavior.
2. Encode Section `7.1` internal orchestration states without ambiguous duplicate ownership.
3. Preserve spec tick order from Section `8.1` and algorithm order from Section `16.2`.
4. Keep dispatch eligibility, blocker gating, and concurrency decisions deterministic and unit
   testable.
5. Leave clean extension seams for:
   - retry scheduling (`KAT-231`)
   - active-run reconciliation and stall detection (`KAT-232`)
   - startup cleanup and release edge cases (`KAT-233`)
   - observability aggregation and snapshots (`KAT-235`)
   - conformance coverage (`KAT-236`)

## Non-Goals

- Fully implementing retry timer scheduling and backoff logic in this ticket
- Fully implementing active-run reconciliation/stall detection internals in this ticket
- Startup terminal cleanup implementation details
- CLI lifecycle behavior
- Observability sink implementation beyond defining the orchestrator-owned state/events they read

## Assumptions

1. `KAT-225` remains the owner of startup and per-tick dispatch preflight validation behavior.
2. `KAT-226` remains the owner of normalized tracker issue shape and candidate/state-refresh query
   behavior.
3. `KAT-229` remains the owner of worker-attempt execution semantics and deterministic worker exit
   reasons.
4. The orchestrator can rely on a single process-local event loop as the serialization boundary for
   all state mutation.
5. Timer callbacks, worker exits, and codex updates are routed back through the orchestrator rather
   than mutating state directly.
6. Late worker completions may arrive after reconciliation or a later tick has already released the
   issue; those completions must log a release intent and must not recreate a running claim.

## Approaches Considered

### Option 1: Narrow orchestration core only

Implement only tick ordering, claim bookkeeping, candidate sorting, and direct dispatch in
`KAT-230`. Leave retry and reconciliation structure mostly undefined until `KAT-231` and `KAT-232`.

Pros:

- Smallest immediate scope

Cons:

- Splits architecture ownership across three tickets
- High risk of contract drift in state/release semantics
- Harder to write stable conformance tests

### Option 2: `KAT-230` as orchestration architecture anchor (recommended)

Define the authoritative runtime state model, event model, dispatch and claim rules, and the seams
that downstream retry/reconciliation/observability tickets consume.

Pros:

- Single source of truth for orchestration state semantics
- Downstream tickets become focused implementation work instead of overlapping redesign
- Cleaner Section `17.4` conformance mapping

Cons:

- Slightly broader design scope for this ticket

### Option 3: Collapse most of M3 into `KAT-230`

Make `KAT-230` own nearly all retry, reconciliation, cleanup, and observability behavior outright.

Pros:

- Fastest path to one large integrated implementation

Cons:

- Breaks current milestone slicing
- Weakens the value of downstream tickets
- Increases delivery and review risk

## Selected Approach

Use **Option 2**. `KAT-230` defines the orchestrator state machine and mutation model; downstream
tickets implement their specific policy mechanisms against those contracts.

## Proposed Design

### 1. Real orchestrator module and ownership model

Replace the no-op shell with a concrete orchestrator implementation that owns all mutable scheduling
state. The orchestrator is the only component allowed to mutate:

- `poll_interval_ms`
- `max_concurrent_agents`
- `running`
- `claimed`
- `retry_attempts`
- `completed`
- aggregate codex totals
- latest codex rate limits

External actors do not mutate these structures directly. They emit events back to the orchestrator.

### 2. Event-loop mutation contract

Define an internal event model, for example:

- `tick`
- `worker_exit`
- `codex_update`
- `retry_requested`
- `retry_timer_fired`
- `reconciliation_result`
- `release_requested`
- `shutdown`

All asynchronous sources route through this contract:

- worker-attempt completion
- streamed codex events
- retry timer callbacks
- reconciliation/stall evaluation

This is the core safety rule for duplicate-dispatch prevention: one mutation authority, one
serialized event path.

### 3. Runtime state model

Introduce a concrete `OrchestratorRuntimeState` plus stable entry types:

- `RunningEntry`
  - normalized issue
  - human identifier
  - worker/session handles
  - retry attempt number
  - started timestamp
  - latest codex/session metadata
  - live token counters for this run
- `RetryEntry`
  - `issue_id`
  - `identifier`
  - `attempt`
  - `due_at_ms`
  - `timer_handle`
  - `error`

Section `7.1` internal claim states are **derived** from state membership, not separately persisted.
The implementation settled on spec-aligned state names instead of composite labels:

- `Unclaimed`
  - issue absent from `claimed`, `running`, `retry_attempts`, and `completed`
- `Claimed`
  - issue reserved in `claimed` with no more active ownership state present
- `Running`
  - issue present in `running`
- `RetryQueued`
  - issue present in `retry_attempts`
- `Released`
  - issue absent from live ownership structures and retained only in completion bookkeeping

When memberships overlap, the implementation prefers the most active state:
`Running -> RetryQueued -> Claimed -> Released -> Unclaimed`.

This keeps release semantics explicit without introducing a separately persisted claim-state field.

### 4. Tick control spine

The tick sequence must match Section `8.1` and Section `16.2` exactly:

1. Reconcile current running set.
2. Run per-tick dispatch preflight validation.
3. Fetch active candidates.
4. Sort candidates for dispatch.
5. Dispatch eligible issues until slots are exhausted.
6. Emit updated observability state.
7. Schedule the next tick using the current effective poll interval.

If preflight validation fails, reconciliation has already happened; candidate fetch and dispatch are
skipped for that tick.

### 5. Pure decision helpers for dispatch

Keep dispatch policy mostly pure and unit testable via dedicated helpers:

- `sortCandidatesForDispatch(issues)`
- `isCandidateStructurallyEligible(issue)`
- `passesTodoBlockerGate(issue, terminalStates)`
- `hasAvailableGlobalSlot(state)`
- `hasAvailableStateSlot(issueState, state)`
- `shouldDispatch(issue, state)`

Rules:

- Sort by `priority` ascending, then `created_at` oldest first, then `identifier` lexicographically.
- Reject issues that are missing required structural fields.
- Reject issues already in `running` or `claimed`.
- Enforce global and per-state limits consistently.
- For `Todo`, reject when any blocker is non-terminal.

`dispatchIssue` becomes the only function that converts an eligible issue into a claimed running
entry.

### 6. Dispatch contract and spawn handoff

`dispatchIssue(issue, attempt)` should:

1. request worker execution for the issue/attempt,
2. on success:
   - create a `RunningEntry`
   - add the issue to `claimed`
   - remove any prior retry entry for that issue
3. on worker spawn failure:
   - emit a retry request rather than mutating retry state inline

This keeps dispatch behavior aligned with Section `16.4` while preserving the event-loop mutation
rule.

### 7. Downstream retry seam (`KAT-231`)

`KAT-230` defines the contract that:

- normal worker exit requests a continuation retry,
- abnormal worker exit requests a failure retry,
- worker exits observed after the issue has already been released stay as release intents with null
  retry metadata,
- release removes the issue from `claimed` plus any associated retry bookkeeping,
- retry re-dispatch must respect the same eligibility and slot rules as normal tick dispatch.

`KAT-231` then implements:

- timer lifecycle
- backoff calculation
- timer replacement/cancellation
- slot exhaustion requeue
- idempotency under timer races

`KAT-231` should not redefine who owns retry state or release semantics.

### 8. Downstream reconciliation seam (`KAT-232`)

`KAT-230` defines the contract that reconciliation runs before dispatch on every tick and may
produce these actions:

- update running issue snapshot
- terminate running issue with workspace cleanup
- terminate running issue without workspace cleanup
- request retry due to stall/abnormal termination
- release claim when appropriate

`KAT-232` then implements:

- stall timeout evaluation against codex event timestamps
- tracker refresh for running issues
- terminal/non-active/active state classification
- termination signal/cleanup decision mechanics

`KAT-232` should not redefine running-entry ownership or the release contract.

### 9. Startup cleanup and release seam (`KAT-233`)

This design declares release semantics centrally in `KAT-230`. `KAT-233` should therefore narrow to:

- startup terminal workspace cleanup
- safe claim release for absent or ineligible retry candidates
- cleanup behavior for terminal issues using workspace manager safeguards

It should not become the primary owner of state-machine release meaning.

### 10. Observability seam (`KAT-235`)

`KAT-230` owns the canonical runtime state and event stream that observability reads from. `KAT-235`
should consume:

- `running` rows
- `retry` rows
- aggregate token/runtime counters
- latest rate-limit snapshot
- state transition events/log fields

This keeps observability derivative rather than authoritative.

## Downstream Ticket Scope Adjustments

### `KAT-231`

Update scope to say it implements retry scheduling **against `KAT-230` retry request and release
contracts**, including timer races and requeue semantics, rather than defining retry ownership.

### `KAT-232`

Update scope to say it implements stall detection and running-issue reconciliation **against
`KAT-230` running-entry and termination/release contracts**.

### `KAT-233`

Update scope to clarify that startup cleanup and absent/ineligible retry-candidate release are
follow-through on `KAT-230` release semantics, not a separate definition of release behavior.

### `KAT-235`

Update scope to clarify that logging and snapshot aggregation consume the orchestrator runtime state
model defined by `KAT-230`.

### `KAT-236`

Update acceptance language or execution notes so conformance tests map directly to `KAT-230`,
`KAT-231`, and `KAT-232` seams.

## Testing Strategy

Focus `KAT-230` on deterministic orchestrator tests before broader integration:

1. Section `7.1` state derivation:
   - unclaimed
   - claimed/running
   - claimed/retryqueued
   - released
2. Sort order:
   - priority ascending
   - oldest `created_at`
   - identifier tie-breaker
3. `Todo` blocker gate:
   - non-terminal blocker rejects
   - terminal blockers allow dispatch
4. Global concurrency enforcement
5. Per-state concurrency enforcement
6. Duplicate suppression for already claimed/running issues
7. Dispatch success path creates claimed running entry
8. Dispatch spawn failure emits retry request contract
9. Codex updates aggregate into both running entry and global totals
10. Tick ordering:
    - reconcile before validation
    - validation before fetch
    - fetch before dispatch

## Risks and Mitigations

1. Risk: retry/reconciliation tickets still reintroduce direct state mutation.
   Mitigation: define the event-loop mutation rule explicitly in contracts and tests.
2. Risk: release semantics drift across tickets.
   Mitigation: derive states from map/set membership and keep release as one canonical transition.
3. Risk: observability layer starts owning counters independently.
   Mitigation: make `KAT-235` consume orchestrator-owned aggregates only.
4. Risk: integration code becomes too coupled to a specific timer/process implementation.
   Mitigation: keep decision helpers pure and isolate handles behind entry types and event adapters.

## Open Follow-Through

- `KAT-230` implementation should land the runtime state contracts and concrete orchestrator shell
  first, before retry/reconciliation specialized behavior expands.
- `KAT-231`, `KAT-232`, `KAT-233`, `KAT-235`, and `KAT-236` should be updated to reference the
  orchestration seams defined here.
