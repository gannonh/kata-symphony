---
type: Spec
title: Github Projects V2 State Source Of Truth Design
description: "Archived spec document: Github Projects V2 State Source Of Truth Design."
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---

# GitHub Projects v2 State Source Of Truth Design

**Date:** 2026-05-06
**Status:** Approved for written spec review
**Scope:** Fix GitHub Projects v2 CLI progress state and remove embedded entity metadata from normal CLI read/write behavior.

## Problem

`project.getSnapshot` can report completed GitHub Projects v2 work as pending when issue body metadata is stale.

The production failure was found on a Kata-planned GitHub Projects v2 project. All slices in a milestone were complete in GitHub, but `/kata-progress` recommended more slice execution because the CLI adapter read stale body metadata such as `status:"backlog"`.

GitHub issue state and Project v2 fields already carry the workflow data the CLI needs. The CLI currently writes those fields, then also writes embedded metadata into issue bodies. Those data sources can drift.

## Goals

1. `project.getSnapshot` derives GitHub Projects v2 slice, task, and standalone issue status from GitHub issue state and Project v2 `Status`.
2. Closed GitHub issues always map to `done`.
3. Open GitHub issues map from Project v2 `Status`.
4. Project v2 custom fields carry Kata identity and classification data.
5. Issue bodies contain user-facing content only.
6. GitHub native issue dependency relationships remain the source for blockers.
7. `/kata-progress` recommends `kata-complete-milestone` when an active milestone has all roadmap slices present, all slices/tasks done, all tasks verified, and no missing required coverage.

## Non-Goals

1. Linear backend work.
2. PR #501 changes.
3. A migration, repair, or backfill command.
4. Reading embedded metadata as a compatibility path.
5. Using `Kata Blocking` or `Kata Blocked By` Project v2 fields.

## Source Model

The GitHub Projects v2 adapter treats GitHub and Project v2 as the control plane:

1. Milestones use native GitHub milestones plus a milestone issue for Kata content and artifacts.
2. Slices are GitHub issues assigned to the native milestone and classified through Project v2 fields.
3. Tasks are GitHub sub-issues of slice issues.
4. Dependencies use native GitHub issue dependency relationships.
5. Status uses GitHub issue state first, then Project v2 `Status` for open issues.
6. Verification uses `Kata Verification State`.
7. Artifact lookup uses `Kata Artifact Scope`.

Required Project v2 fields:

1. `Status`
2. `Kata Type`
3. `Kata ID`
4. `Kata Parent ID`
5. `Kata Artifact Scope`
6. `Kata Verification State`

`Kata Blocking` and `Kata Blocked By` are not required because dependencies use GitHub native relationships.

## Adapter Behavior

Discovery loads Project v2 items and field values. The adapter builds tracked entities from:

1. Project item content issue.
2. `Kata ID`.
3. `Kata Type`.
4. `Kata Parent ID` where the current domain model needs an ID reference.
5. Native GitHub issue milestone assignment.
6. Native GitHub sub-issue relationships for tasks.
7. Native GitHub dependencies for blockers.

The adapter does not require `<!-- kata:entity ... -->` markers for normal discovery.

Status mapping:

1. If the GitHub issue state is `closed`, return `done`.
2. If the issue is open, map Project v2 `Status`.
3. If an open issue has no Project v2 `Status`, return `backlog` and surface the missing field through health checks.
4. Never read status from issue body content.

Writes:

1. Create and update operations write Project v2 fields.
2. Create operations write clean issue bodies containing only the user-facing content.
3. Status updates change Project v2 `Status` and GitHub issue state.
4. Status updates do not rewrite issue bodies for metadata.

## Snapshot Flow

`project.getSnapshot` stays domain-level and trusts adapter-returned state.

1. `getActiveMilestone()` returns the active milestone or `null`.
2. If there is no active milestone, `nextAction.workflow` is `kata-new-milestone`.
3. If there is an active milestone, the snapshot reads milestone artifacts, roadmap, requirements, slices, tasks, dependencies, and verification state.
4. Readiness derives from the adapter-returned slice/task state.
5. `nextAction.workflow` is `kata-complete-milestone` when:
   - all roadmap slices exist,
   - all slices are `done`,
   - all tasks are `done`,
   - all tasks are verified,
   - and no required requirement IDs are missing coverage.

The active production bug is fixed when closed slice issues make `allSlicesDone` true.

## Health And Setup

`kata doctor` validates the fields the adapter uses:

1. `Status` exists with expected workflow options.
2. Required Kata text fields exist with the correct type.
3. `Kata Blocking` and `Kata Blocked By` are not required.

Project setup creates or validates the same field set.

Health output should warn when existing Project v2 items that appear to be Kata records are missing required field values.

## Testing

Add GitHub Projects v2 tests that prove:

1. A closed slice with stale body `status:"backlog"` returns `done`.
2. A closed task with stale body `status:"backlog"` returns `done`.
3. An open slice maps status from Project v2 `Status`.
4. Project v2 `Kata ID` and `Kata Type` discover entities without body metadata.
5. New project, milestone, slice, task, and issue bodies do not include `kata:entity` metadata.
6. Status updates do not rewrite issue bodies to update metadata.
7. Native dependency behavior remains covered.
8. `project.getSnapshot` returns `kata-complete-milestone` for a completed and verified milestone.

Validation sequence:

1. `pnpm --filter @kata-sh/cli exec vitest run src/tests/github-projects-v2.adapter.vitest.test.ts`
2. `pnpm --filter @kata-sh/cli exec vitest run src/tests/golden-path.pi-github.vitest.test.ts`
3. `pnpm --filter @kata-sh/cli test`
4. `pnpm run validate:affected`

## Implementation Handoff

Implementation planning should split the work into:

1. Project v2 field discovery and entity construction.
2. Status precedence tests and adapter fixes.
3. Marker-free create/update writes.
4. Health/setup field validation cleanup.
5. Snapshot completion regression tests.
