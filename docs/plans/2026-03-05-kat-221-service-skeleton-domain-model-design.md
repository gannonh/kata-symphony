# KAT-221 Service Skeleton and Core Domain Model Design

## Context

- Ticket: KAT-221
- Goal: Bootstrap a compile/runnable service skeleton with normalized core domain entities from `SPEC.md` Section 4.
- Project/Epic context reviewed:
  - Linear project: `Symphony Service v1 (Spec Execution)`
  - Milestone: `M1 Foundations & Contract Loading`
  - Dependency DAG doc: `Symphony v1 Execution Plan (Dependency DAG)`
- Blocker status:
  - `KAT-255` is `Done` (required blocker cleared)
- Downstream issues unblocked by this work:
  - `KAT-222`, `KAT-223`, `KAT-224`

## References Reviewed

- `SPEC.md` Section 3 (system overview and layers)
- `SPEC.md` Section 4 (core domain model and normalization rules)
- `WORKFLOW.md` (runtime contract shape and current defaults)
- Linear document: `Project Spec`
- Linear document: `Symphony v1 Execution Plan (Dependency DAG)`
- Existing scaffold baseline from `KAT-255` (`src/main.ts`, TypeScript toolchain scripts)

## Assumptions

1. This ticket defines contracts and module seams only; no dispatch/reconciliation logic is implemented yet.
2. Domain models should match spec names/fields closely to reduce ambiguity in conformance mapping.
3. Runtime boundaries should support parallel implementation lanes without cross-layer imports.

## Options Considered

1. Spec-native interface-first model (selected)
   - Define plain TypeScript interfaces/types per Section 4 entities, plus minimal constructors/helpers.
   - Pros: fastest path to conformance clarity, low coupling, easy testing, ideal for downstream lanes.
   - Cons: behavior methods are postponed to later tickets.

2. Rich domain object model
   - Define classes per entity with methods and invariants built in.
   - Pros: encapsulates behavior near data model.
   - Cons: higher design overhead now, risks locking in behavior before Section 7-12 implementation details stabilize.

3. Orchestrator-first skeleton with inlined types
   - Build orchestrator flow shell first and introduce types opportunistically.
   - Pros: quick end-to-end compile path.
   - Cons: weak normalization guarantees, poorer separation for tracker/workspace/agent lane parallelism.

## Selected Approach

Use **spec-native interface-first contracts** with a dependency-wired module skeleton. Keep models and seams stable now, then layer behavior in subsequent tickets.

## Proposed Module Boundaries (Section 3-Aligned)

### 1. `src/config/`

- Responsibility: typed configuration view and loader contract.
- Exposes:
  - `ConfigSnapshot` type (effective runtime values)
  - `ConfigProvider` interface (get/reload/watch signatures)
- Does not import orchestrator or execution implementations.

### 2. `src/tracker/`

- Responsibility: issue tracker access and payload normalization.
- Exposes:
  - `TrackerClient` interface
  - normalized `Issue` reads (`fetchCandidates`, `fetchIssueStatesByIds`, `fetchTerminalIssues`)
- Depends on domain types only.

### 3. `src/orchestrator/`

- Responsibility: runtime orchestration state contracts and future coordination logic.
- Exposes:
  - `OrchestratorRuntimeState`
  - `RetryEntry`
  - `RunningEntry` (or equivalent runtime row type)
  - `Orchestrator` interface (`start`, `stop`, `tick`)
- Depends on `config`, `tracker`, `execution`, `observability` interfaces.

### 4. `src/execution/`

- Responsibility: workspace and agent runtime contracts.
- Sub-boundaries:
  - `workspace` (path/key/create/remove contracts)
  - `agent` (run-attempt/session protocol contracts)
- Exposes:
  - `WorkspaceManager` interface
  - `AgentRunner` interface
  - `RunAttempt` and `LiveSession` types

### 5. `src/observability/`

- Responsibility: structured logging and optional status/snapshot contracts.
- Exposes:
  - `Logger` interface
  - snapshot model interfaces consumed by operators/TUI later
- Must not mutate orchestration behavior.

### 6. `src/bootstrap/` (wiring shell)

- Responsibility: dependency assembly and startup path only.
- Exposes:
  - `createService()` wiring function
  - `startService()` no-op bootstrap sequence (config load + dependency validation + startup log)

## Domain Model Contract (Section 4 Mapping)

Define these canonical domain contracts under `src/domain/` (or equivalent shared module):

1. `Issue`
   - Fields map 1:1 to Section 4.1.1 (`id`, `identifier`, `title`, `description`, `priority`, `state`, `branch_name`, `url`, `labels`, `blocked_by`, `created_at`, `updated_at`).
   - `labels` normalized to lowercase by tracker adapter.
   - `blocked_by` contains `{id, identifier, state}`.

2. `WorkflowDefinition`
   - `{ config: Record<string, unknown>; prompt_template: string }`

3. `Workspace`
   - `{ path: string; workspace_key: string; created_now: boolean }`

4. `RunAttempt`
   - `{ issue_id, issue_identifier, attempt, workspace_path, started_at, status, error? }`

5. `LiveSession`
   - Fields from Section 4.1.6, including token counters and latest event metadata.

6. `RetryEntry`
   - `{ issue_id, identifier, attempt, due_at_ms, timer_handle, error }`

7. `OrchestratorRuntimeState`
   - `{ poll_interval_ms, max_concurrent_agents, running, claimed, retry_attempts, completed, codex_totals, codex_rate_limits }`

## Normalization Rules to Encode Early

1. Workspace key sanitization uses `[A-Za-z0-9._-]` allowlist replacement.
2. Issue state comparisons use normalized `trim().toLowerCase()`.
3. Session ID convention uses `<thread_id>-<turn_id>`.
4. Issue ID is authoritative key for maps/sets; issue identifier is human-facing/log-facing.

## Startup and Dependency Wiring Shell

Baseline startup flow for this ticket:

1. `src/main.ts` calls `startService()`.
2. `startService()`:
   - creates config provider
   - assembles tracker/execution/observability stubs through constructor-injected interfaces
   - builds orchestrator shell with empty/no-op run loop implementation
   - emits bootstrap success log and exits/returns cleanly (no dispatch loop yet)
3. No tracker polling, workspace actions, or agent launches are enabled in this ticket.

## Testing and Verification Plan

### Contract tests

1. Type-level shape assertions for each Section 4 entity (compile-time and minimal runtime fixtures).
2. Module boundary tests ensuring cross-layer imports follow allowed dependency direction.
3. Startup smoke test confirms wiring path compiles/runs without orchestration enabled.

### Verification commands

1. `pnpm run lint`
2. `pnpm run typecheck`
3. `pnpm test`
4. `pnpm start` (bootstrap smoke)
5. `make check`

## Scope Boundaries

### In Scope

- Core domain entity contracts from Section 4
- Section 3-aligned module seam definitions
- Bootstrap dependency wiring shell and runnable startup path

### Out of Scope

- Orchestrator scheduling, retry algorithms, reconciliation behavior
- Workflow parser/config reload implementation details (`KAT-222`, `KAT-223`)
- Prompt render strictness implementation (`KAT-224`)
- Real tracker and app-server execution behavior

## Risks and Mitigations

1. Risk: Drift from spec field names.
   - Mitigation: keep spec-native field names in core contracts; add adapters later if ergonomic wrappers are needed.
2. Risk: Boundary leakage between lanes.
   - Mitigation: enforce dependency direction with explicit interface modules and lint/test checks.
3. Risk: Overbuilding runtime behavior in a planning ticket.
   - Mitigation: keep startup shell no-op and defer state machine logic to integration tickets.

## Handoff

This design establishes the skeleton and contract surface needed to implement KAT-221 and unlock KAT-222/KAT-223/KAT-224 independently.
