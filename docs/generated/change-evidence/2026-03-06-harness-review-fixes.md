# Change Evidence: harness-review-fixes

## Summary

Address PR review comments: fix merge-base detection, exit code handling, path traversal guards, string validation, env isolation, and topic sanitization.

## Changed Files

- `.githooks/pre-push`
- `docs/generated/change-evidence/2026-03-06-harness-contract-task-4.md`
- `docs/generated/change-evidence/2026-03-06-harness-review-fixes.json`
- `docs/generated/change-evidence/2026-03-06-harness-review-fixes.md`
- `scripts/ci-local.sh`
- `scripts/harness/check_decision_links.sh`
- `scripts/harness/check_doc_relevance.sh`
- `scripts/harness/check_evidence_contract.sh`
- `scripts/harness/check_stale_context.sh`
- `scripts/harness/generate_change_evidence.ts`
- `tests/harness/check-evidence-contract.test.ts`

## Context Loaded

- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/change-evidence-schema.md`
- `docs/harness/context-map.yaml`

## Decision Artifacts

- `docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md`

## Canonical Docs Updated

- None. Review fixes correct existing behavior without changing contract semantics.

## Waivers

- `docs/harness/BUILDING-WITH-HARNESS.md`: Review fixes do not change harness contract semantics, only correctness of existing behavior.

## Verification

- `pnpm vitest run tests/harness/` -> pass
- `make check` -> pass
- `pnpm run typecheck` -> pass

## Verification Artifacts

- None.

## Impacted Areas

- `documentation`
- `harness`
- `tests`
