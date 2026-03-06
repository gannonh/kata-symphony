# Change Evidence: harness-contract-task-4

## Summary

Enforce evidence artifacts and linked decision records for qualifying harness changes.

## Changed Files

- `docs/generated/README.md`
- `docs/generated/change-evidence/2026-03-06-harness-contract-task-4.json`
- `docs/generated/change-evidence/2026-03-06-harness-contract-task-4.md`
- `docs/generated/harness-contract-task-4-verification.md`
- `docs/harness/change-evidence-schema.md`
- `docs/harness/context-map.yaml`
- `scripts/ci-local.sh`
- `scripts/harness/check_decision_links.sh`
- `scripts/harness/check_evidence_contract.sh`
- `scripts/harness/generate_change_evidence.ts`
- `tests/harness/check-evidence-contract.test.ts`
- `tests/harness/generate-change-evidence.test.ts`

## Context Loaded

- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/change-evidence-schema.md`
- `docs/harness/context-map.yaml`
- `docs/harness/context-map.yaml`

## Decision Artifacts

- `docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-implementation-plan.md`

## Canonical Docs Updated

- `docs/harness/change-evidence-schema.md`

## Waivers

- None.

## Verification

- `pnpm vitest tests/harness/check-evidence-contract.test.ts tests/harness/generate-change-evidence.test.ts` -> pass
- `bash scripts/ci-local.sh` -> pass

## Verification Artifacts

- `docs/generated/harness-contract-task-4-verification.md`

## Impacted Areas

- `documentation`
- `harness`
- `tests`
