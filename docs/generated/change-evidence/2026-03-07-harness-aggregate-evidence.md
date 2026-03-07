# Change Evidence: harness-aggregate-evidence

## Summary

Aggregate evidence for the full harness PR including all review fix rounds.

## Changed Files

- `.codex/skills/symphony-harness-evidence/SKILL.md`
- `.githooks/pre-commit`
- `.githooks/pre-push`
- `AGENTS.md`
- `docs/generated/build-time-harness-verification.md`
- `docs/generated/change-evidence/.gitkeep`
- `docs/generated/change-evidence/2026-03-06-build-time-harness.json`
- `docs/generated/change-evidence/2026-03-06-build-time-harness.md`
- `docs/generated/change-evidence/2026-03-06-harness-ci-shellcheck-fix.json`
- `docs/generated/change-evidence/2026-03-06-harness-ci-shellcheck-fix.md`
- `docs/generated/change-evidence/2026-03-06-harness-contract-task-4.json`
- `docs/generated/change-evidence/2026-03-06-harness-contract-task-4.md`
- `docs/generated/change-evidence/2026-03-06-harness-review-fixes.json`
- `docs/generated/change-evidence/2026-03-06-harness-review-fixes.md`
- `docs/generated/change-evidence/2026-03-06-harness-review-round2.json`
- `docs/generated/change-evidence/2026-03-06-harness-review-round2.md`
- `docs/generated/change-evidence/2026-03-06-harness-skill-task-6.json`
- `docs/generated/change-evidence/2026-03-06-harness-skill-task-6.md`
- `docs/generated/change-evidence/2026-03-06-stale-context-task-7.json`
- `docs/generated/change-evidence/2026-03-06-stale-context-task-7.md`
- `docs/generated/change-evidence/2026-03-07-harness-aggregate-evidence.json`
- `docs/generated/change-evidence/2026-03-07-harness-aggregate-evidence.md`
- `docs/generated/harness-contract-task-4-verification.md`
- `docs/generated/harness-skill-task-6-verification.md`
- `docs/generated/README.md`
- `docs/generated/stale-context-task-7-verification.md`
- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/change-evidence-schema.md`
- `docs/harness/context-map.yaml`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-implementation-plan.md`
- `package.json`
- `PLANS.md`
- `QUALITY_SCORE.md`
- `scripts/ci-local.sh`
- `scripts/harness/check_decision_links.sh`
- `scripts/harness/check_doc_relevance.sh`
- `scripts/harness/check_evidence_contract.sh`
- `scripts/harness/check_repo_contract.sh`
- `scripts/harness/check_stale_context.sh`
- `scripts/harness/common.sh`
- `scripts/harness/generate_change_evidence.ts`
- `scripts/install-githooks.sh`
- `tests/harness/check-doc-relevance.test.ts`
- `tests/harness/check-evidence-contract.test.ts`
- `tests/harness/check-stale-context.test.ts`
- `tests/harness/generate-change-evidence.test.ts`
- `tests/scaffold/package-scripts.test.ts`

## Context Loaded

- `AGENTS.md`
- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/change-evidence-schema.md`
- `docs/harness/context-map.yaml`
- `PLANS.md`
- `QUALITY_SCORE.md`

## Decision Artifacts

- `docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md`

## Canonical Docs Updated

- `AGENTS.md`
- `docs/harness/change-evidence-schema.md`
- `docs/harness/context-map.yaml`
- `PLANS.md`
- `QUALITY_SCORE.md`

## Waivers

- `docs/harness/BUILDING-WITH-HARNESS.md`: Harness builder guide is part of this PR's deliverables.

## Verification

- `pnpm vitest run tests/harness/` -> pass
- `make check` -> pass
- `pnpm run typecheck` -> pass
- `pnpm run lint` -> pass

## Verification Artifacts

- None.

## Impacted Areas

- `agents`
- `documentation`
- `harness`
- `planning`
- `quality`
- `tests`
