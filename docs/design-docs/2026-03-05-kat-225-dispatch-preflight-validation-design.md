# KAT-225 Startup and Per-Tick Dispatch Preflight Validation Design

## Context

- Ticket: `KAT-225`
- Branch: `feature/kat-225-plan-add-startup-and-per-tick-dispatch-preflight-validation`
- Goal: implement scheduler preflight validation behavior from `SPEC.md` Section `6.3`.
- Project: `Symphony Service v1 (Spec Execution)`
- Milestone: `M1 Foundations & Contract Loading`

Dependency/relationship context reviewed:

- Blockers (`KAT-222`, `KAT-223`, `KAT-224`) are `Done`.
- This ticket blocks `KAT-230` (`[Plan] Implement orchestrator poll loop, claim state, and dispatch selection`).
- Parallel worktree contract comment (2026-03-05) scopes this ticket to dispatch preflight only.
- No explicit parent epic is attached directly to `KAT-225` in current issue metadata.

Issue-level attachments/documents reviewed:

- None attached directly to `KAT-225`.
- No design mocks attached/referenced for this ticket.

## References Reviewed

- Linear issue: `KAT-225` description + comment thread contract
- Linear docs:
  - `Project Spec` (`c02d5eb5a8d1`)
  - `Symphony v1 Execution Plan (Dependency DAG)` (`5b235d1e8099`)
- Repository docs:
  - `SPEC.md` Section `6.3`, `8.1`, `14.2`, `16.1`, `16.2`, `17.1`, `17.6`, `18.1`
  - `ARCHITECTURE.md`
  - `RELIABILITY.md`
  - `SECURITY.md`
- Existing dependency designs:
  - `docs/plans/2026-03-05-kat-222-workflow-discovery-parser-design.md`
  - `docs/design-docs/2026-03-05-kat-223-typed-config-layer-design.md`
  - `docs/design-docs/2026-03-05-kat-224-strict-prompt-template-rendering-design.md`
- Current code boundaries:
  - `src/orchestrator/contracts.ts`
  - `src/config/contracts.ts`
  - `src/workflow/contracts.ts`, `src/workflow/errors.ts`
  - `src/domain/models.ts`
  - `src/bootstrap/service.ts`, `src/bootstrap/main-entry.ts`

## Problem Statement

Symphony needs a deterministic preflight gate that validates dispatch prerequisites both:

1. At startup, before scheduling begins.
2. On every tick, before dispatch.

Behavior must satisfy Section `6.3`:

- Startup validation failure: fail startup cleanly.
- Per-tick validation failure: skip dispatch for that tick, keep reconciliation active, keep service healthy.
- Validation output must be typed and machine-branchable.
- Operator-visible logs must not leak secrets.

## Required Outcomes

1. Validate all required preflight checks:
   - workflow parseability
   - `tracker.kind` present/supported
   - resolved `tracker.api_key` present
   - required `tracker.project_slug` present
   - `codex.command` non-empty
2. Startup path fails cleanly with typed failure surface.
3. Tick path skips dispatch only; reconciliation still runs.
4. Logging is operator-visible and redacted.

## Scope Boundaries

In scope:

- Preflight validation logic under `src/orchestrator/**`
- Startup and tick preflight tests
- Typed error/result contract for preflight consumers

Out of scope:

- Tracker API calls (`KAT-226`)
- Workspace lifecycle/hooks (`KAT-227`)
- Codex protocol runtime (`KAT-228`)
- Signature changes to frozen contracts (`src/tracker/contracts.ts`, `src/execution/contracts.ts`, `src/domain/models.ts`)

## Assumptions

1. `ConfigProvider.getSnapshot()` from `KAT-223` remains the only config read dependency for this ticket.
2. Workflow loader/parser typed errors from `KAT-222` are available and branchable.
3. Existing logger interface is used for operator-visible output; structured fields are preferred over formatted strings.
4. Per-tick preflight is evaluated after reconciliation and before candidate fetch/dispatch as defined by spec tick order.

## Options Considered

### Option 1: Orchestrator-owned typed preflight validator (recommended)

Implement a dedicated orchestrator preflight module that returns a discriminated union result and shared typed error taxonomy used by both startup and per-tick paths.

Pros:

- Single validation authority for both startup and tick.
- Strong machine-branchable contract with no string parsing.
- Keeps ownership aligned with ticket boundary (`src/orchestrator/**`).

Cons:

- Requires new orchestrator module surface before full orchestrator implementation exists.

### Option 2: Bootstrap-only validator with ad-hoc tick checks

Add startup checks in bootstrap and duplicate subset checks in tick loop once orchestrator is implemented.

Pros:

- Fastest initial startup implementation.

Cons:

- Duplication and likely drift between startup/tick behavior.
- Weak long-term maintainability.

### Option 3: Config-layer validation only

Push all validation into config provider and have orchestrator consume pass/fail only.

Pros:

- Centralized config logic.

Cons:

- Mixes scheduler gating semantics into config layer.
- Harder to represent workflow-loader and dispatch-specific policy outcomes cleanly.

## Selected Approach

Use **Option 1**: build an orchestrator-owned `validateDispatchPreflight` contract with typed result categories, then integrate it into startup and tick control flow.

## Proposed Design

### 1. Typed preflight contract

Create an orchestrator preflight contract (module naming illustrative):

- `DispatchPreflightResult`
  - `{ ok: true }`
  - `{ ok: false; errors: DispatchPreflightError[] }`
- `DispatchPreflightError`
  - `code` (stable enum)
  - `message` (operator-safe summary)
  - `field` (optional config/workflow path)
  - `source` (`workflow` | `config`)

Proposed stable error codes:

- `workflow_invalid`
- `tracker_kind_missing`
- `tracker_kind_unsupported`
- `tracker_api_key_missing`
- `tracker_project_slug_missing`
- `codex_command_missing`

Notes:

- `workflow_invalid` wraps loader parse/read typed failures without forcing caller string parsing.
- Multi-error responses are allowed so operators see complete invalid state in one pass.

### 2. Validation algorithm

`validateDispatchPreflight(...)` performs, in order:

1. Load/validate workflow definition (parseability gate).
2. Read effective config snapshot.
3. Validate tracker kind present/supported.
4. Validate resolved tracker API key non-empty.
5. Validate required project slug for selected tracker kind (`linear`).
6. Validate codex command trimmed/non-empty.

Return:

- `ok: true` when all checks pass.
- `ok: false` with typed errors when any check fails.

### 3. Startup integration

Startup behavior:

1. Run preflight before scheduling loop starts.
2. If `ok: false`:
   - log operator-visible error summary (redacted)
   - surface typed startup failure to caller
   - fail startup cleanly (non-zero via existing main-entry flow)
3. If `ok: true`:
   - continue startup sequence.

### 4. Per-tick integration

Tick behavior (spec order preserved):

1. Reconcile running issues.
2. Run preflight.
3. If preflight fails:
   - log operator-visible error summary (redacted)
   - skip candidate fetch and dispatch for this tick
   - keep service alive, schedule next tick normally
4. If preflight succeeds:
   - continue fetch/select/dispatch path.

### 5. Operator-visible logging and secret hygiene

Logging rules:

- Always emit failures at error level with structured context.
- Include:
  - `phase`: `startup` or `tick`
  - `error_codes`: ordered list of typed codes
  - `workflow_path` when relevant
- Do not include:
  - resolved `tracker.api_key`
  - raw secret-bearing environment values
  - full shell command interpolation beyond safe summary

### 6. Integration boundaries for parallel worktrees

This ticket reads but does not modify:

- `src/config/contracts.ts`
- `src/workflow/*` typed loader errors
- `src/domain/models.ts`
- frozen contracts in tracker/execution/domain modules per comment contract

If new cross-module signature need is discovered, capture as follow-up integration delta for `KAT-229`/`KAT-230`.

## Testing Strategy

### Unit tests (preflight validator)

1. Returns `ok: true` for fully valid workflow/config.
2. Maps workflow load/parse errors to `workflow_invalid`.
3. Missing tracker kind -> `tracker_kind_missing`.
4. Unsupported tracker kind -> `tracker_kind_unsupported`.
5. Missing/empty resolved API key -> `tracker_api_key_missing`.
6. Missing project slug for linear -> `tracker_project_slug_missing`.
7. Empty/whitespace codex command -> `codex_command_missing`.
8. Multi-failure input returns deterministic ordered code set.

### Startup behavior tests

1. Preflight failure aborts startup and preserves clean startup error surface.
2. Startup failure path logs operator-visible preflight error without secret values.
3. Successful preflight allows startup continuation.

### Tick behavior tests

1. Reconciliation still runs when tick preflight fails.
2. Dispatch is skipped on preflight failure.
3. Tick continues normal scheduling cadence after failure.
4. Preflight recovery on later tick resumes dispatch.

## Conformance Mapping

- `SPEC.md 6.3`: startup and per-tick dispatch validation semantics
- `SPEC.md 8.1`: reconcile before dispatch; skip dispatch on failed preflight
- `SPEC.md 14.2`: dispatch validation failures keep service alive
- `SPEC.md 16.1/16.2`: startup/tick algorithm hooks for validation
- `SPEC.md 17.1/17.6`: typed validation and operator-visible observability
- `SPEC.md 18.1`: required conformance item for startup/pre-dispatch validation behavior

## Risks and Mitigations

1. Risk: preflight result shape drifts from downstream orchestrator needs.
   - Mitigation: discriminated union with stable error codes and deterministic ordering.
2. Risk: secret leakage in logs during validation failures.
   - Mitigation: centralized redaction-safe logging helper with explicit allowlist fields.
3. Risk: duplicated gating logic as orchestrator evolves.
   - Mitigation: single shared validator used by both startup and tick entrypoints.

## Handoff

This design is ready for implementation planning and execution of `KAT-225` within the scoped preflight gate boundary.
