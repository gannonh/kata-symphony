# Change Evidence: build-time-harness

## Summary

Roll up the evidence-backed build-time harness rollout and the follow-up review
fixes for canonical-doc authorization, diff-scoped evidence selection, and
branch-base validation.

## Changed Files

- `.codex/skills/symphony-harness-evidence/SKILL.md`
- `.githooks/pre-commit`
- `.githooks/pre-push`
- `AGENTS.md`
- `PLANS.md`
- `QUALITY_SCORE.md`
- `docs/generated/README.md`
- `docs/generated/build-time-harness-verification.md`
- `docs/generated/change-evidence/2026-03-06-build-time-harness.json`
- `docs/generated/change-evidence/2026-03-06-build-time-harness.md`
- `docs/generated/change-evidence/2026-03-06-harness-contract-task-4.json`
- `docs/generated/change-evidence/2026-03-06-harness-contract-task-4.md`
- `docs/generated/change-evidence/2026-03-06-harness-review-fixes.json`
- `docs/generated/change-evidence/2026-03-06-harness-review-fixes.md`
- `docs/generated/change-evidence/2026-03-06-harness-skill-task-6.json`
- `docs/generated/change-evidence/2026-03-06-harness-skill-task-6.md`
- `docs/generated/change-evidence/2026-03-06-stale-context-task-7.json`
- `docs/generated/change-evidence/2026-03-06-stale-context-task-7.md`
- `docs/generated/harness-contract-task-4-verification.md`
- `docs/generated/harness-skill-task-6-verification.md`
- `docs/generated/stale-context-task-7-verification.md`
- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/change-evidence-schema.md`
- `docs/harness/context-map.yaml`
- `package.json`
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
- `PLANS.md`
- `QUALITY_SCORE.md`
- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/change-evidence-schema.md`
- `docs/harness/context-map.yaml`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-implementation-plan.md`

## Decision Artifacts

- `docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-implementation-plan.md`

## Canonical Docs Updated

- `AGENTS.md`
- `PLANS.md`
- `QUALITY_SCORE.md`
- `docs/generated/README.md`
- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/change-evidence-schema.md`
- `docs/harness/context-map.yaml`

## Waivers

- `SECURITY.md`: The rollout and review fixes tightened build-time harness rules
  but did not change the repository security posture itself.
- `RELIABILITY.md`: The review fixes tightened local branch validation without
  changing the runtime reliability objectives or failure model.
- `ARCHITECTURE.md`: No product architecture layer or module-boundary contract
  changed; this work stayed in the build-time harness.

## Verification

- `pnpm vitest tests/harness/check-doc-relevance.test.ts` -> pass
- `pnpm vitest tests/harness/check-evidence-contract.test.ts` -> pass
- `bash scripts/ci-local.sh` -> pass
- `make check` -> pass

## Verification Artifacts

- `docs/generated/build-time-harness-verification.md`
- `docs/generated/harness-contract-task-4-verification.md`
- `docs/generated/harness-skill-task-6-verification.md`
- `docs/generated/stale-context-task-7-verification.md`

## Impacted Areas

- `agents`
- `documentation`
- `harness`
- `planning`
- `quality`
- `tests`
