# Change Evidence: stale-context-task-7

## Summary

Add an opt-in stale-context drift audit for subsystem docs, plans, and evidence links.

## Changed Files

- `PLANS.md`
- `QUALITY_SCORE.md`
- `docs/generated/change-evidence/2026-03-06-stale-context-task-7.json`
- `docs/generated/change-evidence/2026-03-06-stale-context-task-7.md`
- `docs/generated/stale-context-task-7-verification.md`
- `docs/harness/context-map.yaml`
- `scripts/harness/check_stale_context.sh`
- `tests/harness/check-stale-context.test.ts`

## Context Loaded

- `PLANS.md`
- `QUALITY_SCORE.md`
- `docs/harness/context-map.yaml`

## Decision Artifacts

- `docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-implementation-plan.md`

## Canonical Docs Updated

- `PLANS.md`
- `QUALITY_SCORE.md`
- `docs/harness/context-map.yaml`

## Waivers

- None.

## Verification

- `pnpm vitest tests/harness/check-stale-context.test.ts` -> pass
- `bash scripts/harness/check_stale_context.sh` -> pass

## Verification Artifacts

- `docs/generated/stale-context-task-7-verification.md`

## Impacted Areas

- `documentation`
- `harness`
- `planning`
- `quality`
