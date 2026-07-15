---
type: Spec
title: Kata Skills Migration Recovery Design
description: Archived spec document: Kata Skills Migration Recovery Design.
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---

# Kata Skills Migration Recovery Design

## Status

The current `apps/cli/skills-src` and generated `apps/cli/skills` should be treated as scaffolding, not a completed workflow migration.

The new CLI/backend runtime is still valuable and should be preserved. The failed part is the skill layer: the generated skills expose folder structure and operation names, but they do not migrate the tactical, battle-tested workflow behavior from `apps/orchestrator-legacy`.

## Problem

The current skills are too abstract for agent execution.

Example: `kata-new-milestone` currently says:

```text
Read project context with project.getContext.
Create the milestone with milestone.create.
Write milestone artifacts with artifact.write.
```

That is not executable guidance for a harness agent. It omits:

- Exact command syntax.
- Payload file creation.
- JSON schemas.
- Expected return shapes.
- User questioning flow.
- Approval gates.
- Artifact title/type/scope conventions.
- Error handling.
- How legacy workflow phases map to backend artifacts.

The result is predictable: agents improvise, skip operations, run the wrong command, or inspect helper scripts instead of executing the workflow.

## Recovery Principle

Migrate behavior, not file shapes.

Each skill must be rebuilt from:

- The legacy command prompt in `apps/orchestrator-legacy/commands/kata/`.
- The referenced legacy workflow in `apps/orchestrator-legacy/kata/workflows/`.
- The referenced legacy templates in `apps/orchestrator-legacy/kata/templates/`.
- The referenced legacy guidance in `apps/orchestrator-legacy/kata/references/`.
- The new CLI typed backend contract in `apps/cli/src/domain/types.ts` and `apps/cli/src/domain/operations.ts`.

The output must be a portable Agent Skill using progressive disclosure, but the progressive disclosure files must contain substantial workflow instructions, not stubs.

## Scope

Do not narrow the Phase A skill surface further. Redo the full current Phase A skill set:

- `kata-setup`
- `kata-health`
- `kata-new-project`
- `kata-new-milestone`
- `kata-plan-phase`
- `kata-execute-phase`
- `kata-verify-work`
- `kata-complete-milestone`
- `kata-progress`

The current generated skill names are acceptable as the Phase A surface. Their contents are not acceptable.

## Non-Goals

- Do not revive the old custom Kata CLI agent.
- Do not reintroduce local markdown files as the durable backend.
- Do not add backend-specific behavior to skill logic.
- Do not validate work by checking only that `SKILL.md` files exist.
- Do not call a skill migrated unless it has been manually exercised through Pi against a real GitHub Projects v2 backend.

## Backend Contract Requirements

Every runtime operation exposed to skills must include tactical instructions in the relevant `runtime-contract.md`:

- Operation name.
- Whether `--input` is required.
- Exact shell command.
- JSON payload schema.
- Concrete JSON example.
- Expected successful response shape.
- Common failure response and recovery guidance.

Example command forms:

```bash
node ./scripts/kata-call.mjs project.getContext
node ./scripts/kata-call.mjs milestone.create --input /tmp/kata-milestone-create.json
node ./scripts/kata-call.mjs artifact.write --input /tmp/kata-artifact-write.json
```

The skill must not rely on the agent inferring that operation names are CLI calls.

## Artifact Contract Requirements

Every durable artifact write must specify:

- `scopeType`
- `scopeId`
- `artifactType`
- `title`
- `format`
- Required markdown sections
- Whether it replaces, appends, or supersedes an earlier artifact

Project-scoped artifacts must use `scopeType: "project"` and `scopeId: "PROJECT"`.

Milestone-scoped artifacts must use `scopeType: "milestone"` and the milestone ID returned from `milestone.create` or `milestone.getActive`.

Slice/task artifacts must use the IDs returned by `slice.create` and `task.create`.

## Progressive Disclosure Layout

Each skill should have this structure after the redo:

```text
apps/cli/skills-src/
  manifest.json
  references/
    cli-runtime.md
    alignment.md
    questioning.md
    artifact-contract.md
    ui-brand.md
  workflows/
    <skill>.md
  templates/
    project.md
    requirements.md
    roadmap.md
    state.md
    phase-prompt.md
    verification-report.md
    milestone-archive.md
  scripts/
    kata-call.mjs
```

Generated skills should copy only the references/templates needed by that skill. `SKILL.md` stays concise and points to the specific files required for the workflow.

## Migration Matrix

| Skill | Legacy source | Required legacy materials | Required backend operations | Migration status |
|---|---|---|---|---|
| `kata-setup` | `commands/kata/health.md`, setup behavior from new CLI | `workflows/health.md`, `references/planning-config.md` where still relevant | `health.check`, `project.getContext` | Redo |
| `kata-health` | `commands/kata/health.md` | `workflows/health.md` | `health.check`, `project.getContext`, artifact/list operations if needed | Redo |
| `kata-new-project` | `commands/kata/new-project.md` | `workflows/new-project.md`, `references/questioning.md`, `references/ui-brand.md`, `templates/project.md`, `templates/requirements.md`, research templates | `health.check`, `project.upsert`, `artifact.write`, likely `artifact.read/list` | Redo |
| `kata-new-milestone` | `commands/kata/new-milestone.md` | `workflows/new-milestone.md`, `references/questioning.md`, `references/ui-brand.md`, `templates/project.md`, `templates/requirements.md`, research templates | `project.getContext`, `artifact.read/list`, `milestone.create`, `artifact.write` | Redo |
| `kata-plan-phase` | `commands/kata/plan-phase.md` | `workflows/plan-phase.md`, `templates/phase-prompt.md`, `templates/discovery.md`, `references/phase-argument-parsing.md`, `references/model-profile-resolution.md` | `milestone.getActive`, `artifact.read/list/write`, `slice.create`, `task.create` | Redo |
| `kata-execute-phase` | `commands/kata/execute-phase.md` | `workflows/execute-phase.md`, `workflows/execute-plan.md`, `templates/summary*.md`, `references/checkpoints.md`, `references/continuation-format.md` | `milestone.getActive`, `slice.list`, `task.list`, `task.updateStatus`, `artifact.read/write`, `execution.getStatus` | Redo |
| `kata-verify-work` | `commands/kata/verify-work.md` | `workflows/verify-work.md`, `templates/UAT.md`, `templates/verification-report.md`, `references/verification-patterns.md` | `milestone.getActive`, `slice.list`, `task.list`, `task.updateStatus`, `artifact.read/list/write` | Redo |
| `kata-complete-milestone` | `commands/kata/complete-milestone.md` | `workflows/complete-milestone.md`, `templates/milestone-archive.md`, `templates/retrospective.md` | `milestone.getActive`, `artifact.read/list/write`, `milestone.complete` | Redo |
| `kata-progress` | `commands/kata/progress.md` | `workflows/progress.md`, `references/continuation-format.md` | `project.getContext`, `milestone.getActive`, `slice.list`, `task.list`, `artifact.list`, `execution.getStatus` | Redo |

## Legacy Capability Inventory

These legacy commands must not silently disappear. They can be:

- Migrated as separate skills in a later phase.
- Integrated into one of the Phase A skills.
- Explicitly marked out of current phase with rationale.

| Legacy command | Initial disposition |
|---|---|
| `discuss-phase` | Integrate relevant behavior into `kata-plan-phase`; no standalone discuss skill. |
| `research-phase` | Integrate relevant behavior into `kata-plan-phase`; optional standalone later. |
| `validate-phase` | Integrate into `kata-verify-work` or later validation skill. |
| `audit-milestone` | Candidate later skill; related to `kata-complete-milestone`. |
| `map-codebase` | Candidate later skill; relevant to new-project brownfield path. |
| `quick` | Candidate later skill; not part of Phase A core chain. |
| `add-phase`, `insert-phase`, `remove-phase` | Candidate later roadmap-editing skills. |
| `add-todo`, `check-todos` | Candidate later todo/intake skills. |
| `add-tests` | Candidate later verification/testing skill. |
| `settings`, `set-profile` | Candidate later configuration skills; may partially move to CLI setup. |
| `pause-work`, `resume-work` | Candidate later continuity skills. |
| `cleanup`, `update`, `help`, `join-discord`, `debug`, `reapply-patches`, `list-phase-assumptions`, `plan-milestone-gaps` | Candidate later utility/support skills. |

## Skill Definition Of Done

A skill is migrated only when all of these are true:

- `SKILL.md` has a clear trigger description and concise execution rules.
- Workflow reference contains tactical, step-by-step agent behavior migrated from the legacy workflow.
- Runtime contract includes executable command examples for every backend operation.
- Every required payload has schema and example JSON.
- Every expected response shape is shown.
- Every durable artifact has a naming/scope/type convention.
- Legacy workflow gates are either preserved or explicitly adapted.
- Legacy templates are adapted into skill templates or artifact schemas.
- Tests assert the generated skill includes the required workflow, runtime, and template references.
- Manual Pi run exercises the skill against GitHub Projects v2 and records evidence.

## Manual Acceptance Gate

The Phase A migration is not complete until a tester can start Pi in this monorepo and execute:

```text
kata-health
kata-new-project
kata-new-milestone
kata-plan-phase
kata-execute-phase
kata-verify-work
kata-complete-milestone
kata-progress
```

against the configured GitHub Projects v2 backend.

Acceptance evidence must include:

- Commands the agent ran.
- JSON payload files or payload excerpts.
- GitHub issue/project URLs created or updated.
- Artifact readback proof.
- Agent transcript notes for any user-facing workflow gates.
- Any failures and the fix before re-running.

## Required CLI Follow-Ups

The skill redo may expose backend contract gaps. Known likely gaps:

- `project.getContext` currently returns config identity, not durable project artifact content.
- Skills need `artifact.read/list` for project and milestone artifacts before many workflows can be faithful.
- Some legacy workflows imply settings/config operations that may need typed CLI operations.
- Some execution workflows may require Symphony delegation or explicit "manual harness execution" instructions.

These are not reasons to make the skill vague. If a workflow needs an operation, add it to the typed CLI contract or explicitly adapt the workflow.

## Implementation Strategy

1. Build shared migration infrastructure first: CLI runtime reference, artifact contract reference, operation examples, template copying.
2. Migrate each Phase A skill from its legacy source with a per-skill migration checklist.
3. Add tests that fail if generated skills are skeleton-only.
4. Rebuild and reinstall Pi skills after each migrated skill.
5. Manually validate each skill before marking it complete.

