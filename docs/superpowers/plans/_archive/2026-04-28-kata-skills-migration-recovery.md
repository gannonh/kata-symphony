---
type: Plan
title: Kata Skills Migration Recovery
description: "Archived implementation plan: Kata Skills Migration Recovery."
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---

# Kata Skills Migration Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the Phase A Kata skills as real workflow migrations from `apps/orchestrator-legacy`, with executable CLI instructions and Pi + GitHub Projects v2 validation evidence.

**Architecture:** Keep the new `apps/cli` backend/runtime contract. Replace the current skill skeleton content with progressive-disclosure skills derived from legacy workflows, references, and templates. Enforce migration quality with tests and manual Pi acceptance instead of "file exists" checks.

**Tech Stack:** Agent Skills, TypeScript, Node.js, pnpm, Vitest, Pi coding agent, GitHub Projects v2, `@kata-sh/cli`

**Spec:** `docs/superpowers/specs/2026-04-28-kata-skills-migration-recovery-design.md`

---

## Current State

Current generated skills are scaffolding. Do not treat them as migrated.

Current dirty changes from the timeout fix are separate from this plan:

- `.env.example`
- `apps/cli/skills-src/scripts/kata-call.mjs`
- generated `apps/cli/skills/*/scripts/kata-call.mjs`
- `apps/cli/src/tests/build-skill-bundle.vitest.test.ts`

Decide whether to commit those helper fixes before starting this plan to keep history clean.

## File Map

### Create

| File | Responsibility |
|---|---|
| `apps/cli/skills-src/references/cli-runtime.md` | Exact command patterns, payload file rules, success/error response handling. |
| `apps/cli/skills-src/references/artifact-contract.md` | Artifact scope/type/title/content conventions for project, milestone, slice, and task artifacts. |
| `apps/cli/skills-src/references/questioning.md` | Adapted legacy questioning protocol for project and milestone discovery. |
| `apps/cli/skills-src/references/ui-brand.md` | Portable stage banners and user-facing formatting from legacy UI guidance. |
| `apps/cli/skills-src/templates/project.md` | Backend artifact version of legacy project template. |
| `apps/cli/skills-src/templates/requirements.md` | Backend artifact version of legacy requirements template. |
| `apps/cli/skills-src/templates/roadmap.md` | Backend artifact version of legacy roadmap template. |
| `apps/cli/skills-src/templates/state.md` | Backend artifact version of legacy state template. |
| `apps/cli/skills-src/templates/phase-prompt.md` | Backend artifact version of legacy phase prompt template. |
| `apps/cli/skills-src/templates/verification-report.md` | Backend artifact version of legacy verification report template. |
| `apps/cli/skills-src/templates/milestone-archive.md` | Backend artifact version of legacy milestone archive template. |
| `apps/cli/src/tests/skill-migration-quality.vitest.test.ts` | Guards against skeletal workflow/runtime references. |
| `docs/superpowers/specs/2026-04-28-kata-skills-migration-status.md` | Per-skill migration and validation matrix. |

### Modify

| File | Change |
|---|---|
| `apps/cli/scripts/bundle-skills.mjs` | Copy per-skill references/templates and fail if required materials are missing. |
| `apps/cli/skills-src/manifest.json` | Declare required references/templates per skill. |
| `apps/cli/skills-src/workflows/*.md` | Replace skeleton workflow prose with migrated legacy behavior. |
| `apps/cli/skills-src/references/alignment.md` | Keep, but ensure it supports legacy questioning gates rather than replacing them. |
| `apps/cli/src/tests/build-skill-bundle.vitest.test.ts` | Assert generated skills include real workflow/reference/template material. |
| `docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md` | Replace with concrete Pi validation steps per migrated skill. |
| `apps/cli/src/domain/types.ts` | Add operations/types only if a faithful migrated skill needs them. |
| `apps/cli/src/domain/operations.ts` | Add validation for new operations only when required by migrated workflows. |
| `apps/cli/src/commands/call.ts` | Keep command behavior simple; improve error messages if skill validation shows agent confusion. |

## Task 1: Commit Or Isolate The Timeout Helper Fix

**Files:**
- Existing dirty files listed in Current State

- [ ] **Step 1: Inspect dirty state**

Run:

```bash
git status --short
git diff --stat
```

Expected: only timeout-helper changes plus the new recovery spec/plan if this plan has not been committed.

- [ ] **Step 2: Decide commit boundary**

If committing the timeout helper fix separately, stage only:

```bash
git add .env.example apps/cli/skills-src/scripts/kata-call.mjs apps/cli/skills/*/scripts/kata-call.mjs apps/cli/src/tests/build-skill-bundle.vitest.test.ts
```

Do not stage `.env`.

- [ ] **Step 3: Verify helper fix**

Run:

```bash
pnpm --dir apps/cli run test:vitest -- src/tests/build-skill-bundle.vitest.test.ts src/tests/env.vitest.test.ts
node /Users/gannonhall/.pi/agent/skills/kata-new-project/scripts/kata-call.mjs health.check
```

Expected: tests pass and `health.check` returns JSON without launching legacy interactive Kata CLI.

- [ ] **Step 4: Commit helper fix separately before skill migration**

Run:

```bash
git commit -m "fix(cli): let installed skills use local cli root"
```

## Task 2: Add Shared Runtime And Artifact References

**Files:**
- Create: `apps/cli/skills-src/references/cli-runtime.md`
- Create: `apps/cli/skills-src/references/artifact-contract.md`
- Modify: `apps/cli/scripts/bundle-skills.mjs`
- Modify: `apps/cli/src/tests/build-skill-bundle.vitest.test.ts`

- [ ] **Step 1: Create `cli-runtime.md`**

Add concrete instructions for:

```bash
node ./scripts/kata-call.mjs project.getContext
node ./scripts/kata-call.mjs health.check
node ./scripts/kata-call.mjs milestone.create --input /tmp/kata-milestone-create.json
node ./scripts/kata-call.mjs artifact.write --input /tmp/kata-artifact-write.json
```

Include these rules:

- Create JSON payload files before required-input operations.
- Use `/tmp/kata-<operation>.json` for temporary payloads unless the harness provides a better scratch path.
- Treat `{ "ok": false }` as a blocking error.
- Do not inspect `scripts/kata-call.mjs` unless the command itself fails.
- Never call abstract operation names without the helper command.

- [ ] **Step 2: Create `artifact-contract.md`**

Document exact artifact conventions:

```json
{
  "scopeType": "project",
  "scopeId": "PROJECT",
  "artifactType": "project-brief",
  "title": "PROJECT",
  "content": "# Project\n\n...",
  "format": "markdown"
}
```

Also include milestone, requirements, roadmap, phase-context, plan, verification, uat, summary, retrospective, and milestone archive conventions.

- [ ] **Step 3: Update bundler**

Modify `apps/cli/scripts/bundle-skills.mjs` so every generated skill receives:

- `references/cli-runtime.md`
- `references/artifact-contract.md`
- `references/alignment.md`
- `scripts/kata-call.mjs`

- [ ] **Step 4: Add bundle test assertions**

Update `apps/cli/src/tests/build-skill-bundle.vitest.test.ts` to assert generated skills include:

```text
references/cli-runtime.md
references/artifact-contract.md
scripts/kata-call.mjs
```

- [ ] **Step 5: Verify**

Run:

```bash
pnpm --dir apps/cli run test:vitest -- src/tests/build-skill-bundle.vitest.test.ts
pnpm --dir apps/cli run build
```

Expected: bundle test passes and generated skills include the shared references.

## Task 3: Add Migration Quality Guardrails

**Files:**
- Create: `apps/cli/src/tests/skill-migration-quality.vitest.test.ts`
- Modify: `apps/cli/skills-src/manifest.json`

- [ ] **Step 1: Add per-skill source metadata**

Extend each manifest entry with:

```json
{
  "legacyCommand": "apps/orchestrator-legacy/commands/kata/new-project.md",
  "legacyWorkflow": "apps/orchestrator-legacy/kata/workflows/new-project.md",
  "requiredReferences": [
    "cli-runtime",
    "artifact-contract",
    "questioning",
    "ui-brand"
  ],
  "requiredTemplates": [
    "project",
    "requirements"
  ]
}
```

- [ ] **Step 2: Write quality test**

Create a test that fails when any workflow contains only skeleton phrases without command recipes. Minimum assertions:

- Each workflow has at least one `scripts/kata-call.mjs` command block when runtime operations are declared.
- Each declared operation appears in `runtime-contract.md` with a JSON example when it requires input.
- Each skill's legacy command and workflow paths exist.
- `kata-new-project` workflow contains `questioning`, `project.upsert`, `artifact.write`, and `kata-new-milestone`.
- `kata-new-milestone` workflow contains `milestone.create`, requirements artifact guidance, roadmap artifact guidance, and `kata-plan-phase`.
- `kata-plan-phase` workflow contains `slice.create`, `task.create`, plan artifact guidance, and phase gate language.

- [ ] **Step 3: Verify failing state**

Run:

```bash
pnpm --dir apps/cli run test:vitest -- src/tests/skill-migration-quality.vitest.test.ts
```

Expected before migration: fails against current skeletons.

Do not weaken this test to pass skeletons.

## Task 4: Migrate `kata-health` And `kata-setup`

**Files:**
- Modify: `apps/cli/skills-src/workflows/health.md`
- Modify: `apps/cli/skills-src/workflows/setup.md`
- Modify: `apps/cli/skills-src/manifest.json`
- Modify: `docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md`

- [ ] **Step 1: Read legacy sources**

Read:

```bash
apps/orchestrator-legacy/commands/kata/health.md
apps/orchestrator-legacy/kata/workflows/health.md
```

- [ ] **Step 2: Rewrite health workflow**

The migrated workflow must:

- Run `node ./scripts/kata-call.mjs health.check`.
- Run `node ./scripts/kata-call.mjs project.getContext`.
- Explain each check in user-facing language.
- Give actionable fixes for missing token/config/backend.
- Avoid launching setup unless health shows setup is missing.

- [ ] **Step 3: Rewrite setup workflow**

The migrated workflow must:

- Tell Pi users to run `node apps/cli/dist/loader.js setup --pi` from the monorepo during local dev.
- Tell npm users to run `npx @kata-sh/cli setup --pi` or installed `kata setup --pi`.
- Verify with `doctor` and `health.check`.

- [ ] **Step 4: Validate in Pi**

Run Pi with `kata-health` and record the transcript summary in the manual runbook.

## Task 5: Migrate `kata-new-project`

**Files:**
- Modify: `apps/cli/skills-src/workflows/new-project.md`
- Create/modify: `apps/cli/skills-src/references/questioning.md`
- Create/modify: `apps/cli/skills-src/templates/project.md`
- Modify: `apps/cli/skills-src/manifest.json`
- Modify if needed: `apps/cli/src/domain/*`

- [ ] **Step 1: Read legacy sources**

Read:

```bash
apps/orchestrator-legacy/commands/kata/new-project.md
apps/orchestrator-legacy/kata/workflows/new-project.md
apps/orchestrator-legacy/kata/references/questioning.md
apps/orchestrator-legacy/kata/templates/project.md
apps/orchestrator-legacy/kata/templates/requirements.md
```

- [ ] **Step 2: Define adapted scope**

Adapt legacy behavior to backend artifacts:

- Preserve deep questioning and "ready to create project" gate.
- Preserve greenfield/brownfield awareness where supported.
- Persist project context through `project.upsert`.
- Persist project brief through `artifact.write` with `artifactType: "project-brief"`.
- Persist initial requirements hypotheses through `artifact.write` with `artifactType: "requirements"` if gathered.
- Do not create a milestone.
- End by telling the user to run `kata-new-milestone`.

- [ ] **Step 3: Add executable command examples**

Include complete examples for:

```bash
node ./scripts/kata-call.mjs project.upsert --input /tmp/kata-project-upsert.json
node ./scripts/kata-call.mjs artifact.write --input /tmp/kata-project-brief.json
```

- [ ] **Step 4: Validate in Pi**

Run `kata-new-project` against the real GitHub backend. Evidence must include:

- Project prompt flow.
- `project.upsert` payload.
- `artifact.write` payload.
- GitHub project tracking issue URL.
- Artifact readback proof.

## Task 6: Migrate `kata-new-milestone`

**Files:**
- Modify: `apps/cli/skills-src/workflows/new-milestone.md`
- Modify: `apps/cli/skills-src/templates/requirements.md`
- Modify: `apps/cli/skills-src/templates/roadmap.md`
- Modify if needed: `apps/cli/src/domain/*`

- [ ] **Step 1: Read legacy sources**

Read:

```bash
apps/orchestrator-legacy/commands/kata/new-milestone.md
apps/orchestrator-legacy/kata/workflows/new-milestone.md
apps/orchestrator-legacy/kata/references/questioning.md
apps/orchestrator-legacy/kata/templates/requirements.md
apps/orchestrator-legacy/kata/templates/roadmap.md
apps/orchestrator-legacy/kata/templates/state.md
```

- [ ] **Step 2: Rewrite workflow**

The migrated workflow must:

- Read project context and project artifacts.
- Gather milestone goal using legacy questioning behavior.
- Define scoped requirements with IDs.
- Create exactly one milestone with `milestone.create`.
- Write requirements and roadmap milestone artifacts.
- End with `kata-plan-phase`.

- [ ] **Step 3: Add executable command examples**

Include exact commands and payloads for:

```bash
node ./scripts/kata-call.mjs project.getContext
node ./scripts/kata-call.mjs artifact.read --input /tmp/kata-read-project-brief.json
node ./scripts/kata-call.mjs milestone.create --input /tmp/kata-milestone-create.json
node ./scripts/kata-call.mjs artifact.write --input /tmp/kata-requirements.json
node ./scripts/kata-call.mjs artifact.write --input /tmp/kata-roadmap.json
```

- [ ] **Step 4: Validate in Pi**

Run `kata-new-milestone` after `kata-new-project`. Evidence must include milestone ID, GitHub URL, requirements artifact, roadmap artifact, and handoff to `kata-plan-phase`.

## Task 7: Migrate `kata-plan-phase`

**Files:**
- Modify: `apps/cli/skills-src/workflows/plan-phase.md`
- Create/modify: `apps/cli/skills-src/templates/phase-prompt.md`
- Create/modify: `apps/cli/skills-src/templates/discovery.md`

- [ ] **Step 1: Read legacy sources**

Read:

```bash
apps/orchestrator-legacy/commands/kata/plan-phase.md
apps/orchestrator-legacy/kata/workflows/plan-phase.md
apps/orchestrator-legacy/kata/templates/phase-prompt.md
apps/orchestrator-legacy/kata/templates/discovery.md
```

- [ ] **Step 2: Rewrite workflow**

The migrated workflow must:

- Load active milestone.
- Read milestone requirements/roadmap artifacts.
- Select the phase/slice to plan.
- Perform integrated discussion/research where legacy requires it.
- Create slices and tasks with `slice.create` and `task.create`.
- Write plan artifacts.
- Present execution-ready output.

- [ ] **Step 3: Validate in Pi**

Run `kata-plan-phase` and prove it creates executable slices/tasks in GitHub Projects v2.

## Task 8: Migrate `kata-execute-phase`

**Files:**
- Modify: `apps/cli/skills-src/workflows/execute-phase.md`
- Create/modify: `apps/cli/skills-src/templates/summary.md`
- Create/modify: `apps/cli/skills-src/templates/summary-standard.md`
- Create/modify: `apps/cli/skills-src/templates/summary-complex.md`

- [ ] **Step 1: Read legacy sources**

Read:

```bash
apps/orchestrator-legacy/commands/kata/execute-phase.md
apps/orchestrator-legacy/kata/workflows/execute-phase.md
apps/orchestrator-legacy/kata/workflows/execute-plan.md
apps/orchestrator-legacy/kata/templates/summary.md
```

- [ ] **Step 2: Rewrite workflow**

The migrated workflow must:

- Load active milestone, slices, tasks, and plan artifacts.
- Explain whether execution is manual harness work or Symphony-managed work in this phase.
- Update task status through `task.updateStatus`.
- Write summary artifacts.
- Preserve legacy wave/dependency behavior where the harness can support it.

- [ ] **Step 3: Validate in Pi**

Run `kata-execute-phase` on a small UAT repository task and record task transitions and summary artifact evidence.

## Task 9: Migrate `kata-verify-work`

**Files:**
- Modify: `apps/cli/skills-src/workflows/verify-work.md`
- Create/modify: `apps/cli/skills-src/templates/verification-report.md`
- Create/modify: `apps/cli/skills-src/templates/UAT.md`

- [ ] **Step 1: Read legacy sources**

Read:

```bash
apps/orchestrator-legacy/commands/kata/verify-work.md
apps/orchestrator-legacy/kata/workflows/verify-work.md
apps/orchestrator-legacy/kata/templates/UAT.md
apps/orchestrator-legacy/kata/templates/verification-report.md
```

- [ ] **Step 2: Rewrite workflow**

The migrated workflow must:

- Load tasks and plan/summary artifacts.
- Run conversational UAT one test at a time.
- Record pass/fail/blocked outcomes.
- Update task verification state.
- Write `uat` or `verification` artifacts.

- [ ] **Step 3: Validate in Pi**

Run `kata-verify-work` and record UAT artifact evidence.

## Task 10: Migrate `kata-complete-milestone`

**Files:**
- Modify: `apps/cli/skills-src/workflows/complete-milestone.md`
- Create/modify: `apps/cli/skills-src/templates/milestone-archive.md`
- Create/modify: `apps/cli/skills-src/templates/retrospective.md`

- [ ] **Step 1: Read legacy sources**

Read:

```bash
apps/orchestrator-legacy/commands/kata/complete-milestone.md
apps/orchestrator-legacy/kata/workflows/complete-milestone.md
apps/orchestrator-legacy/kata/templates/milestone-archive.md
apps/orchestrator-legacy/kata/templates/retrospective.md
```

- [ ] **Step 2: Rewrite workflow**

The migrated workflow must:

- Load active milestone.
- Check requirements/task completion.
- Surface incomplete work before closure.
- Write summary, retrospective, and archive artifacts.
- Call `milestone.complete`.
- End with `kata-new-milestone` as the next-cycle option.

- [ ] **Step 3: Validate in Pi**

Run `kata-complete-milestone` and record milestone completion evidence.

## Task 11: Migrate `kata-progress`

**Files:**
- Modify: `apps/cli/skills-src/workflows/progress.md`

- [ ] **Step 1: Read legacy sources**

Read:

```bash
apps/orchestrator-legacy/commands/kata/progress.md
apps/orchestrator-legacy/kata/workflows/progress.md
```

- [ ] **Step 2: Rewrite workflow**

The migrated workflow must:

- Load project context.
- Load active milestone.
- List slices, tasks, and relevant artifacts.
- Summarize progress in user-facing language.
- Recommend the next skill based on state.

- [ ] **Step 3: Validate in Pi**

Run `kata-progress` after at least one milestone/slice/task exists and record output.

## Task 12: Final Full-Chain Acceptance

**Files:**
- Modify: `docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md`
- Modify: `docs/superpowers/specs/2026-04-28-kata-skills-migration-status.md`

- [ ] **Step 1: Build and install skills**

Run:

```bash
pnpm --dir apps/cli run build
node apps/cli/dist/loader.js setup --pi
```

- [ ] **Step 2: Run full CLI tests**

Run:

```bash
pnpm --dir apps/cli run test:vitest
```

Expected: all CLI tests pass.

- [ ] **Step 3: Run manual Pi chain**

From the monorepo root, run Pi and exercise:

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

- [ ] **Step 4: Record evidence**

Update the status doc with:

- Date.
- Harness/model.
- GitHub repo/project used.
- Created milestone/slice/task URLs.
- Artifact titles and readback proof.
- Transcript summary.
- Failures and fixes.

- [ ] **Step 5: Commit completed migration**

Stage only files changed for the migration:

```bash
git add apps/cli/skills-src apps/cli/skills apps/cli/scripts/bundle-skills.mjs apps/cli/src/tests docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md docs/superpowers/specs/2026-04-28-kata-skills-migration-status.md
git commit -m "fix(cli): migrate kata skills from legacy workflows"
```
