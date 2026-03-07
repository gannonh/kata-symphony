# Change Evidence: harness-review-round2

## Summary

Address second round of PR review comments: derive canonical docs from context-map, fix || true inconsistency, improve markdown path checks, and decouple tests from live context-map.

## Changed Files

- `docs/generated/change-evidence/2026-03-06-harness-review-fixes.json`
- `docs/generated/change-evidence/2026-03-06-harness-review-fixes.md`
- `docs/generated/change-evidence/2026-03-06-harness-review-round2.json`
- `docs/generated/change-evidence/2026-03-06-harness-review-round2.md`
- `scripts/harness/check_decision_links.sh`
- `scripts/harness/check_doc_relevance.sh`
- `tests/harness/check-doc-relevance.test.ts`
- `tests/harness/generate-change-evidence.test.ts`

## Context Loaded

- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/change-evidence-schema.md`
- `docs/harness/context-map.yaml`

## Decision Artifacts

- `docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md`

## Canonical Docs Updated

- None. Review fixes correct existing behavior without changing contract semantics.

## Waivers

- `docs/harness/BUILDING-WITH-HARNESS.md`: Review fixes correct existing behavior without changing harness contract semantics.

## Verification

- `pnpm vitest run tests/harness/` -> pass
- `make check` -> pass

## Verification Artifacts

- None.

## Impacted Areas

- `documentation`
- `harness`
- `tests`
