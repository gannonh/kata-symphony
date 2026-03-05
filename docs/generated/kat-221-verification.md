# KAT-221 Verification Evidence

Date: 2026-03-05
Issue: KAT-221
Branch: `feature/kat-221-plan-bootstrap-service-skeleton-and-core-domain-model`

## Acceptance Criteria Mapping

1. Domain model fields map to Section 4 entities.
- Evidence:
  - `src/domain/models.ts` defines `Issue`, `WorkflowDefinition`, `Workspace`, `RunAttempt`, `LiveSession`, `RetryEntry`, and `OrchestratorRuntimeState` contracts using spec-native field names.
  - `tests/domain/core-domain-models.test.ts` validates contract usability with concrete fixtures.
  - `tests/domain/normalization.test.ts` validates Section 4.2 normalization rules, including dot-segment and empty-key path-safety guards.

2. Module boundaries support independent tracker/workspace/agent lane implementation.
- Evidence:
  - `src/config/contracts.ts`
  - `src/tracker/contracts.ts`
  - `src/execution/contracts.ts`
  - `src/orchestrator/contracts.ts`
  - `src/observability/contracts.ts`
  - `tests/contracts/layer-contracts.test.ts` verifies contract exports and prevents domain-layer coupling.

3. Basic startup path compiles/runs without orchestration logic enabled.
- Evidence:
  - `src/bootstrap/service.ts` provides `createService()` and `startService()` using no-op orchestrator.
  - `src/bootstrap/main-entry.ts` provides guarded startup handling (`runMain`) to avoid unhandled rejections on startup failure, including defensive handling if error reporting itself fails.
  - `src/main.ts` starts service through bootstrap shell.
  - `tests/bootstrap/service-wiring.test.ts` validates startup path.
  - `tests/bootstrap/main-entry.test.ts` validates startup-failure handling path.
  - `tests/bootstrap/startup.test.ts` validates `pnpm start` exits successfully and emits bootstrap message.

## Command Evidence

- `pnpm vitest tests/domain/core-domain-models.test.ts`
  - PASS
- `pnpm vitest tests/domain/normalization.test.ts tests/domain/core-domain-models.test.ts`
  - PASS
- `pnpm vitest tests/contracts/layer-contracts.test.ts`
  - PASS
- `pnpm vitest tests/contracts/config-provider.test.ts`
  - PASS
- `pnpm vitest tests/bootstrap/service-wiring.test.ts tests/bootstrap/startup.test.ts`
  - PASS
- `pnpm vitest tests/bootstrap/main-entry.test.ts`
  - PASS
- `pnpm start`
  - PASS (`Symphony bootstrap ok` printed)
- `pnpm run lint`
  - PASS
- `pnpm run typecheck`
  - PASS
- `pnpm test`
  - PASS
- `make check`
  - PASS
