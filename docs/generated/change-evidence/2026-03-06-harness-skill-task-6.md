# Change Evidence: harness-skill-task-6

## Summary

Add repo-local skill guidance for evidence-backed harness work and register it in the repo map.

## Changed Files

- `.codex/skills/symphony-harness-evidence/SKILL.md`
- `AGENTS.md`
- `docs/generated/change-evidence/2026-03-06-harness-skill-task-6.json`
- `docs/generated/change-evidence/2026-03-06-harness-skill-task-6.md`
- `docs/generated/harness-skill-task-6-verification.md`
- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/context-map.yaml`

## Context Loaded

- `AGENTS.md`
- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/context-map.yaml`

## Decision Artifacts

- `docs/plans/2026-03-06-build-time-harness-evidence-contract-design.md`
- `docs/plans/2026-03-06-build-time-harness-evidence-contract-implementation-plan.md`

## Canonical Docs Updated

- `AGENTS.md`
- `docs/harness/BUILDING-WITH-HARNESS.md`
- `docs/harness/context-map.yaml`

## Waivers

- None.

## Verification

- `rg -n "symphony-harness-evidence" AGENTS.md docs/harness/BUILDING-WITH-HARNESS.md .codex/skills/symphony-harness-evidence/SKILL.md` -> pass

## Verification Artifacts

- `docs/generated/harness-skill-task-6-verification.md`

## Impacted Areas

- `agents`
- `documentation`
- `harness`
