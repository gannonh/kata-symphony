# Change Evidence: harness-review-fixes

## Summary

Close the final review bypasses in doc authorization, evidence selection, and
local branch-base validation.

## Changed Files

- `.githooks/pre-push`
- `docs/generated/build-time-harness-verification.md`
- `docs/generated/change-evidence/2026-03-06-build-time-harness.json`
- `docs/generated/change-evidence/2026-03-06-build-time-harness.md`
- `docs/generated/change-evidence/2026-03-06-harness-review-fixes.json`
- `docs/generated/change-evidence/2026-03-06-harness-review-fixes.md`
- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/change-evidence-schema.md`
- `scripts/ci-local.sh`
- `scripts/harness/check_decision_links.sh`
- `scripts/harness/check_doc_relevance.sh`
- `scripts/harness/check_evidence_contract.sh`
- `scripts/harness/common.sh`
- `tests/harness/check-doc-relevance.test.ts`
- `tests/harness/check-evidence-contract.test.ts`

## Context Loaded

- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/change-evidence-schema.md`
- `docs/harness/context-map.yaml`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-implementation-plan.md`

## Decision Artifacts

- `docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-implementation-plan.md`

## Canonical Docs Updated

- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/change-evidence-schema.md`

## Waivers

- None yet. Add explicit doc waivers if required.

## Verification

- `pnpm vitest tests/harness/check-doc-relevance.test.ts` -> pass
- `pnpm vitest tests/harness/check-evidence-contract.test.ts` -> pass
- `bash scripts/ci-local.sh` -> pass

## Verification Artifacts

- `docs/generated/build-time-harness-verification.md`

## Impacted Areas

- `documentation`
- `harness`
- `tests`
