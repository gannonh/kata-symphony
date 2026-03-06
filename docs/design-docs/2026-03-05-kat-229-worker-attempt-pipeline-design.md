# KAT-229 Worker Attempt Pipeline Design

## Context

- Ticket: `KAT-229`
- Branch: `feature/kat-229-plan-implement-worker-attempt-pipeline-workspace-prompt`
- Goal: implement `SPEC.md` Section `16.5` worker-attempt algorithm end-to-end.
- Project/epic context reviewed:
  - Linear project: `Symphony Service v1 (Spec Execution)`
  - Milestone: `M2 Parallel Subsystems`
  - Downstream blocked issue: `KAT-230` (orchestrator poll/dispatch core)
- Dependency status verified in start sequence:
  - `KAT-224` `Done`
  - `KAT-226` `Done`
  - `KAT-227` `Done`
  - `KAT-228` `Done`
- Issue docs/attachments/design mocks:
  - No issue-level attachments, docs, or mocks are attached to `KAT-229`.

## References Reviewed

- Linear issue: `KAT-229` (scope, acceptance criteria, blocker graph)
- Related issue for integration seam: `KAT-230`
- Linear project docs:
  - `Project Spec` (`c02d5eb5a8d1`)
  - `Symphony v1 Execution Plan (Dependency DAG)` (`5b235d1e8099`)
- Repository contracts/specs:
  - `SPEC.md` Sections `7.2`, `10.3`, `10.7`, `12`, `16.5`, `16.6`, `17.2`, `17.5`, `18.1`
  - `ARCHITECTURE.md`
  - `WORKFLOW.md`
  - `PLANS.md`, `RELIABILITY.md`, `SECURITY.md`
- Existing implementation/design context:
  - `docs/design-docs/2026-03-05-kat-224-strict-prompt-template-rendering-design.md`
  - `docs/design-docs/2026-03-05-kat-226-linear-tracker-adapter-normalization-design.md`
  - `docs/design-docs/2026-03-05-kat-227-workspace-manager-hooks-safety-design.md`
  - `docs/design-docs/2026-03-05-kat-228-codex-app-server-protocol-runner-design.md`
  - `src/execution/workspace/*`
  - `src/execution/prompt/*`
  - `src/execution/agent-runner/*`
  - `src/tracker/*`
  - `src/execution/contracts.ts`

## Problem Statement

Current code has the three prerequisite building blocks, but no integrated worker-attempt pipeline that does all of the following in one deterministic flow:

1. create/reuse workspace and apply hook semantics,
2. build first-turn vs continuation-turn prompts,
3. run turn loop on a live agent thread,
4. stream codex events to orchestrator-facing callbacks,
5. refresh tracker issue state between turns,
6. stop on non-active state or max-turn bounds,
7. return deterministic normal vs abnormal exit reasons for retry handling.

Without this integration seam, `KAT-230` cannot dispatch real work safely.

## Assumptions and Clarifications

1. `KAT-229` owns worker-attempt execution behavior; full orchestrator state machine and retry queue policy remain in `KAT-230`/`KAT-231`/`KAT-232`.
2. Existing `WorkspaceManager` contract is reused directly for `ensureWorkspace`, `runBeforeRun`, and `runAfterRun`.
3. Existing tracker adapter (`fetchIssuesByIds`) is used for between-turn state refresh.
4. Continuation guidance must not resend the full workflow prompt body on every turn (Section 7 guidance + Section 10 continuation semantics).
5. `after_run` remains best-effort and non-fatal on all post-workspace failure/timeout paths.

## Approaches Considered

1. Keep all worker logic inside `createAgentRunner.runAttempt`
   - Pros: fewer new modules.
   - Cons: mixes workspace, prompt policy, tracker refresh, and protocol concerns into one runtime object; hard to test turn-loop policy independently.

2. Add a dedicated worker-attempt coordinator that composes workspace manager, prompt builder, tracker, and app-server turn client (recommended)
   - Pros: aligns with architecture layer boundaries, keeps deterministic policy surface for Section 16.5, and leaves `agent-runner` focused on protocol execution.
   - Cons: requires new interfaces and small refactor of current `agent-runner` API shape.

3. Push turn-loop and prompt logic up into orchestrator code
   - Pros: orchestrator sees all state transitions directly.
   - Cons: breaks execution-layer boundaries and couples orchestrator internals to codex protocol details.

## Selected Approach

Use **Approach 2**: introduce a dedicated worker-attempt coordinator in the execution layer.

The coordinator owns the Section 16.5 loop and returns explicit terminal semantics to orchestrator code, while delegating specialized concerns to existing modules.

## Proposed Design

### 1. New Execution Seam

Add a worker-attempt module (path names indicative):

- `src/execution/worker-attempt/contracts.ts`
- `src/execution/worker-attempt/run-worker-attempt.ts`
- `src/execution/worker-attempt/index.ts`

Primary API:

- `runWorkerAttempt({ issue, attempt, workflowTemplate, activeStates, maxTurns, onCodexEvent }): Promise<WorkerAttemptResult>`

`WorkerAttemptResult` includes:

- `attempt` (`RunAttempt`) with deterministic status
- `session` (`LiveSession | null`) last known session metadata
- `outcome`:
  - `kind: 'normal' | 'abnormal'`
  - `reason_code: string` (stable machine code)
  - `turns_executed: number`
  - `final_issue_state: string | null`

### 2. Deterministic Exit Reason Contract

Normal reasons:

- `stopped_non_active_state`
- `stopped_max_turns_reached`

Abnormal reasons:

- `workspace_error`
- `before_run_hook_error`
- `agent_session_startup_error`
- `prompt_error`
- `agent_turn_error`
- `issue_state_refresh_error`
- `agent_session_stop_error` (only if stop failure should be classified fatal)

This gives orchestrator retry logic a strict and stable branch key.

### 3. Prompt Policy (First vs Continuation)

Turn `1` prompt:

- Strict render of `workflow.prompt_template` with `issue` and `attempt`.

Turn `>= 2` prompt:

- Strict continuation guidance template that is intentionally short and does not resend full workflow body.
- Includes bounded context only (for example `issue.identifier`, current `turn_number`, `max_turns`, and `attempt`).

Prompt failure semantics:

- Any render failure returns `abnormal/prompt_error` and ends the attempt.

### 4. Worker Attempt Algorithm

1. `ensureWorkspace(issue.identifier)`.
2. `runBeforeRun(workspace)`.
3. Start app-server session in workspace.
4. For `turnNumber` from `1..maxTurns`:
   - Build prompt according to first/continuation rules.
   - Execute one turn on the same session thread; stream protocol updates to `onCodexEvent`.
   - Refresh issue with `tracker.fetchIssuesByIds([issue.id])`.
   - If refresh fails: abnormal exit (`issue_state_refresh_error`).
   - If refreshed issue state is not active: normal exit (`stopped_non_active_state`).
   - If `turnNumber == maxTurns`: normal exit (`stopped_max_turns_reached`).
5. Stop app-server session.
6. Always run `runAfterRun(workspace)` in best-effort mode once workspace exists.

### 5. `after_run` Reliability Rule

`runAfterRun` executes in a guarded `finally` path.

- Failures/timeouts are logged and ignored.
- They do not override the primary attempt outcome.
- This preserves Section 17.2 semantics and issue acceptance requirements.

### 6. Event Streaming and Session Metadata

Per-turn runtime messages are emitted upstream via callback:

- `onCodexEvent({ issue_id, issue_identifier, event, timestamp, payload, session })`

Session updates include:

- `thread_id`, `turn_id`, `session_id`
- token usage/rate-limit deltas if present
- latest event/timestamp/message

This keeps `KAT-230` integration straightforward without coupling orchestrator state internals into execution code.

### 7. Integration with Existing Code

Planned refactor boundaries:

1. Keep `workspace` and `prompt` modules as-is and compose them.
2. Reuse `agent-runner` protocol pieces, but expose a session lifecycle API that supports multiple turns on one thread.
3. Keep tracker usage constrained to `fetchIssuesByIds` for post-turn refresh.
4. Extend `src/execution/contracts.ts` with a worker-attempt runner contract used by orchestrator wiring.

## Conformance Mapping

- Section `7.2`: explicit run lifecycle phases and deterministic terminal reasons.
- Section `12.1`-`12.4`: strict prompt inputs/rules with immediate failure on render errors.
- Section `16.5`: workspace -> before_run -> session -> turn loop -> refresh -> stop conditions -> after_run.
- Section `17.2`: `before_run` fatal, `after_run` best-effort.
- Section `10.3` and `17.5`: reuse same thread across continuation turns; stream runtime events.
- Section `16.6`: emits clear normal/abnormal outcome for orchestrator retry policy.

## Test Strategy

Add focused tests under `tests/execution/worker-attempt/` plus targeted updates in runner tests.

1. Happy path with two turns then non-active refresh stop -> `normal/stopped_non_active_state`.
2. Max-turn stop at boundary -> `normal/stopped_max_turns_reached`.
3. Workspace creation failure -> `abnormal/workspace_error`.
4. `before_run` failure/timeout -> `abnormal/before_run_hook_error`.
5. First-turn prompt render failure -> `abnormal/prompt_error`.
6. Continuation prompt render failure on turn 2+ -> `abnormal/prompt_error`.
7. Agent turn failure/timeout -> `abnormal/agent_turn_error`.
8. Issue refresh failure after successful turn -> `abnormal/issue_state_refresh_error`.
9. `after_run` failure does not change success/failure outcome.
10. Event callback receives streamed protocol messages with issue/session context.
11. Continuation turns reuse thread ID and increment turn count.

Verification targets when implementation starts:

- `pnpm run lint`
- `pnpm run typecheck`
- `pnpm test`
- `make check`

## Scope Boundaries

### In Scope

- Section 16.5 worker-attempt pipeline integration across workspace/prompt/turn loop/state refresh.
- Deterministic normal vs abnormal exit reason contract.
- Best-effort `after_run` behavior on failure/timeout paths.
- Worker-attempt focused tests.

### Out of Scope

- Full orchestrator poll/claim/sort/dispatch state machine (`KAT-230`).
- Retry queue scheduling (`KAT-231`) and stall reconciliation (`KAT-232`).
- Startup terminal workspace cleanup workflow (`KAT-233`).
- Observability dashboard/API surfaces (`KAT-235`).

## Risks and Mitigations

1. Risk: contract overlap between worker-attempt coordinator and existing `agent-runner` API.
   - Mitigation: keep protocol-level responsibilities in `agent-runner`, policy loop in worker-attempt module.
2. Risk: ambiguous classification between timeout/error reason codes.
   - Mitigation: centralize reason-code mapping in one module and freeze codes in tests.
3. Risk: continuation prompt accidentally repeats full workflow body.
   - Mitigation: separate first-turn and continuation template paths with dedicated tests.
4. Risk: `after_run` failures masking primary attempt outcome.
   - Mitigation: primary outcome is finalized before `after_run`; hook failures are logged only.

## Handoff

This design defines the worker-attempt integration contract needed to unblock `KAT-230` while preserving Section `7.2`, `12`, and `16.5` semantics and the required `after_run` best-effort behavior.
