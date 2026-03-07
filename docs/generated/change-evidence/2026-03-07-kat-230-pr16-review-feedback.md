# Change Evidence: kat-230-pr16-review-feedback

## Summary

Address PR #16 review feedback for KAT-230 by hardening orchestrator shutdown, retry bookkeeping, and codex event callback isolation.

## Changed Files

- `ARCHITECTURE.md`
- `docs/generated/change-evidence/2026-03-07-kat-230-pr16-review-feedback.json`
- `docs/generated/change-evidence/2026-03-07-kat-230-pr16-review-feedback.md`
- `docs/plans/2026-03-07-kat-230-orchestrator-poll-loop-claim-state-and-dispatch-design.md`
- `docs/plans/2026-03-07-kat-230-orchestrator-poll-loop-claim-state-and-dispatch-implementation-plan.md`
- `src/bootstrap/service.ts`
- `src/execution/contracts.ts`
- `src/execution/worker-attempt/contracts.ts`
- `src/execution/worker-attempt/run-worker-attempt.ts`
- `src/orchestrator/contracts.ts`
- `src/orchestrator/preflight/run-tick-preflight-gate.ts`
- `src/orchestrator/runtime/contracts.ts`
- `src/orchestrator/runtime/dispatch-selection.ts`
- `src/orchestrator/runtime/index.ts`
- `src/orchestrator/runtime/run-poll-tick.ts`
- `src/orchestrator/runtime/state-machine.ts`
- `src/orchestrator/service.ts`
- `tests/bootstrap/service-internals.test.ts`
- `tests/bootstrap/service-wiring.test.ts`
- `tests/bootstrap/service-worker-attempt-wiring.test.ts`
- `tests/contracts/layer-contracts.test.ts`
- `tests/contracts/runtime-modules.test.ts`
- `tests/execution/worker-attempt/run-worker-attempt.test.ts`
- `tests/execution/worker-attempt/worker-attempt-contracts.test.ts`
- `tests/orchestrator/dispatch-selection.test.ts`
- `tests/orchestrator/run-poll-tick.test.ts`
- `tests/orchestrator/run-tick-preflight-gate.test.ts`
- `tests/orchestrator/runtime-contracts.test.ts`
- `tests/orchestrator/service.test.ts`
- `tests/orchestrator/state-machine.test.ts`

## Context Loaded

- `ARCHITECTURE.md`
- `RELIABILITY.md`
- `SECURITY.md`
- `SPEC.md`
- `docs/plans/2026-03-07-kat-230-orchestrator-poll-loop-claim-state-and-dispatch-design.md`
- `docs/plans/2026-03-07-kat-230-orchestrator-poll-loop-claim-state-and-dispatch-implementation-plan.md`

## Decision Artifacts

- `docs/plans/2026-03-07-kat-230-orchestrator-poll-loop-claim-state-and-dispatch-design.md`
- `docs/plans/2026-03-07-kat-230-orchestrator-poll-loop-claim-state-and-dispatch-implementation-plan.md`

## Canonical Docs Updated

- `ARCHITECTURE.md`

## Waivers

- `SPEC.md`: The review fixes preserve the KAT-230 product contract and do not change the user-visible scope defined in the spec.
- `RELIABILITY.md`: Shutdown now waits for the active tick and worker lifecycle callbacks, but the existing reliability posture already covers serialized orchestrator ownership and recovery expectations.
- `SECURITY.md`: Promise-wrapping codex event callbacks does not change workspace isolation, secret handling, or approval boundaries.

## Verification

- `make check` -> pass
- `pnpm typecheck` -> pass
- `pnpm test` -> pass
- `pnpm test:coverage` -> pass

## Verification Artifacts

- None yet. Add linked verification docs here.

## Impacted Areas

- `architecture`
- `documentation`
- `execution`
- `observability`
- `orchestration`
- `reliability`
- `security`
- `tests`
