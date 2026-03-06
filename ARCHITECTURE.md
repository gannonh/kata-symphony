# Symphony Architecture Map

Last reviewed: 2026-03-05

## Scope

Language-agnostic architecture reference for implementing `SPEC.md`.

## Layers

1. Policy layer
   - `WORKFLOW.md` prompt and runtime policy
2. Configuration layer
   - Workflow loader and typed config coercion
3. Coordination layer
   - Orchestrator state machine (polling, claims, retries, reconciliation)
4. Execution layer
   - Workspace lifecycle and coding-agent session runtime
5. Integration layer
   - Linear-compatible issue tracker client
6. Observability layer
   - Structured logs and optional status surface

## TypeScript Module Map (Current Skeleton)

1. Shared domain contracts
   - `src/domain/models.ts`
   - `src/domain/normalization.ts`
2. Configuration layer contracts
   - `src/config/contracts.ts`
3. Integration layer contracts
   - `src/tracker/contracts.ts`
4. Execution layer contracts
   - `src/execution/contracts.ts`
5. Coordination layer contracts
   - `src/orchestrator/contracts.ts`
6. Observability layer contracts
   - `src/observability/contracts.ts`
7. Bootstrap wiring shell
   - `src/bootstrap/service.ts`
   - `src/bootstrap/main-entry.ts`
   - `src/main.ts`

## Canonical Runtime Flow

1. Load workflow content and build resolved runtime config (env/token/path coercion).
2. Run startup dispatch preflight for dispatch-critical invariants before orchestrator boot.
3. Reconcile in-flight runs.
4. Poll active issues from tracker.
5. Run tick dispatch preflight gate before any dispatch.
6. Select eligible issues with concurrency bounds.
7. Dispatch worker attempts in isolated workspaces.
8. Stream agent runtime events into orchestrator state.
9. Retry or release issues based on exit reason and tracker state.
10. Surface state via logs/status.

## Dispatch Preflight Gating Semantics

1. Startup fail-fast gate
   - `startService` runs `validateDispatchPreflight` before starting the orchestrator.
   - This preflight validates dispatch-readiness invariants (workflow readability plus required resolved config fields), not baseline config coercion.
   - On failure it logs structured preflight errors for `phase: "startup"` and throws `StartupPreflightError`.
   - `runMain` catches startup errors, reports `Symphony startup failed`, and sets `process.exitCode = 1`.
2. Per-tick dispatch gate
   - `runTickPreflightGate` runs reconcile first, then preflight validation.
   - If validation fails, it logs failure for `phase: "tick"` and returns `dispatchAllowed: false`.
   - Dispatch is skipped for that tick; reconciliation still completes.
3. Redacted preflight logging
   - `logPreflightFailure` emits only safe, structured metadata: `phase`, `error_codes`, and sanitized `errors` entries (`code`, `field`, `source`, `message`), plus optional `workflow_path`.
   - Arbitrary context (including secrets like tracker API keys) is not logged.
