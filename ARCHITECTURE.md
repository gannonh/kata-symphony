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

1. Load and validate workflow/config.
2. Reconcile in-flight runs.
3. Poll active issues from tracker.
4. Select eligible issues with concurrency bounds.
5. Dispatch worker attempts in isolated workspaces.
6. Stream agent runtime events into orchestrator state.
7. Retry or release issues based on exit reason and tracker state.
8. Surface state via logs/status.
