---
type: Spec
title: Cli Linear Core Design
description: Archived spec document: Cli Linear Core Design.
tags: [archive, cli]
timestamp: 2026-07-15T20:00:00Z
---

# CLI Linear Core Design

**Date:** 2026-05-06
**Status:** Approved for written spec review
**Scope:** Full Linear backend support in `@kata-sh/cli` for project, milestone, slice, task, standalone issue, artifact, dependency, health, and snapshot operations.

## Problem

Kata needs Linear to work as a full durable backend for CLI planning and execution while preserving the existing GitHub Projects v2 backend. The CLI already exposes a backend-neutral `KataBackendAdapter` contract, but current Linear support is partial and cannot run the full Kata planning and execution workflow.

This spec covers the first independently shippable slice: CLI Linear Core. Symphony execution and PR/land validation follow in later specs.

## Goals

1. Operators can configure `@kata-sh/cli` to use Linear through `.kata/preferences.md`.
2. Linear implements the existing Kata backend contract for project, milestone, slice, task, issue, artifact, health, and snapshot operations.
3. Linear uses native Linear primitives for project structure, workflow state, artifacts, sub-issues, and blocking relations.
4. GitHub Projects v2 behavior remains covered by regression tests.
5. The CLI and Symphony remain independently configurable. CLI reads `.kata/preferences.md`; Symphony reads `WORKFLOW.md`.

## Non-goals

1. Symphony execution changes.
2. PR/land validation fixture work.
3. Live Linear API validation against a real workspace.
4. GitHub Projects v2 behavior changes beyond regression coverage.
5. New domain operation names unless a Linear capability cannot fit the current contract.

## Architecture

`@kata-sh/cli` gains a full `LinearKataAdapter` that implements `KataBackendAdapter`. The adapter maps the backend-neutral Kata domain model onto native Linear records.

Linear mapping:

1. Project: Linear Project.
2. Milestone: Linear Project Milestone.
3. Milestone artifacts: Linear Documents associated with the project and milestone context.
4. Slice: Linear Issue in the project milestone.
5. Task: Linear sub-issue under the slice issue.
6. Standalone Kata issue: Linear Issue.
7. Slice, task, and standalone issue artifacts: marker comments on the associated Linear issue.
8. Dependencies: native Linear blocking relations between slice issues.
9. Statuses: workflow state names from CLI configuration, with canonical Kata stages as defaults.

Kata entity identity and classification come from Linear-native records, configured labels, title ID prefixes, project milestones, parent/sub-issue relationships, workflow state, and native blocking relations. Linear descriptions remain human-readable record content. Linear IDs remain backend provenance and adapter internals. Machine-readable markers are limited to artifact documents and artifact comments.

## Configuration

CLI configuration lives in `.kata/preferences.md`.

Required Linear preferences:

1. `workflow.mode: linear`
2. Linear workspace identifier or slug.
3. Linear team identifier or key.
4. Linear project identifier or slug.
5. Auth source, resolved from `LINEAR_API_KEY`, `LINEAR_TOKEN`, or an environment variable named in preferences.

Optional Linear preferences:

1. State mapping from Kata statuses to Linear workflow state names.
2. Label names used for entity classification and human discoverability. When labels are absent on existing records, the adapter can infer entity type from title ID prefixes, project milestone placement, and parent/sub-issue structure.
3. Active milestone identifier. When this is unset and multiple Linear Project Milestones are eligible, `milestone.getActive` returns an `INVALID_CONFIG` error that asks the operator to pin the active milestone.

The default state names are the canonical Kata stages:

1. `Backlog`
2. `Todo`
3. `In Progress`
4. `Agent Review`
5. `Human Review`
6. `Merging`
7. `Done`

Configured state names are authoritative for the CLI. `kata doctor` validates that every configured state exists for the configured Linear team.

## Data Flow

Setup flow:

1. `kata setup --backend linear` writes `.kata/preferences.md`.
2. Setup asks for or accepts workspace, team, project, auth, and optional state mapping.
3. Setup does not read Symphony `WORKFLOW.md`.

Health flow:

1. `kata doctor` parses `.kata/preferences.md`.
2. It validates Linear auth, workspace, team, project, project milestone support, configured states, documents, comments, sub-issues, and blocking relations.
3. It reports setup-oriented fixes for missing or invalid capability checks.

Planning flow:

1. `project.upsert` creates or updates the Linear Project.
2. `milestone.create` creates a Linear Project Milestone.
3. `artifact.write` with milestone scope writes requirements, roadmap, and related milestone artifacts as Linear Documents.
4. `slice.create` creates a Linear issue in the project milestone, applies configured slice labels, and applies native blocking relations from `blockedBy`.
5. `task.create` creates a Linear sub-issue under the slice issue and applies configured task labels.
6. `issue.create` creates a standalone Linear issue and applies configured issue labels.

Snapshot flow:

1. `project.getSnapshot` reads Linear project context and the active project milestone.
2. It reads milestone documents for requirements, roadmap, and milestone artifacts.
3. It reads slice issues, task sub-issues, workflow state, issue comment artifacts, native blocking relations, and verification evidence.
4. It returns the existing `KataProjectSnapshot` shape, including `roadmap.sliceDependencies`, `roadmap.implementationWaves`, `readiness`, `nextAction`, and `otherActions`.
5. `snapshot.nextAction` remains the source of truth for planning and execution skills.

## Artifacts

Milestone-scoped artifacts use Linear Documents:

1. `requirements`
2. `roadmap`
3. `context`
4. `decisions`
5. `research`
6. `summary`
7. `verification`
8. `uat`
9. `retrospective`

Issue-scoped artifacts use Linear comments with machine-readable markers:

1. Slice artifacts on the slice issue.
2. Task artifacts on the task sub-issue.
3. Standalone issue artifacts on the standalone issue.

Artifact reads and writes must be idempotent by scope and artifact type. Rewriting an artifact updates the existing document or marker comment.

## Dependencies

Linear slice dependencies use native Linear blocking relations as the authoritative backend relationship.

Planning still reads roadmap dependency metadata so it can create backend relations when a planned slice becomes a Linear issue. After a slice is created, `project.getSnapshot` merges these sources:

1. Linear native blocking relations.
2. Roadmap dependency metadata that references existing backend slice IDs.

Execution selection only advances slices whose known blockers are done. If an explicit slice target is blocked, the CLI snapshot must expose the blocker IDs and statuses so skills can stop before state mutation.

## Error Handling

Runtime operations return structured `KataDomainError` failures.

Blocking cases:

1. Missing or invalid Linear auth.
2. Configured workspace, team, project, or project milestone not found.
3. Configured workflow states missing from the Linear team.
4. Required Linear operations unavailable: documents, comments, sub-issues, or blocking relations.
5. Slice dependency references point to unknown backend slice IDs.
6. Runtime operation targets a Linear record whose native classification does not match the requested Kata scope.
7. Milestone artifact writes fail because the milestone document context cannot be resolved.

When `.kata/preferences.md` selects Linear, the CLI must use Linear for durable backend effects. Local runtime storage remains a test fixture path.

## Testing

Add a Linear fake client suite that covers the full backend contract without live API calls.

Required coverage:

1. Config parsing and setup output for `.kata/preferences.md`.
2. Doctor checks for auth, workspace, team, project, state names, documents, comments, sub-issues, and blocking relations.
3. Project and milestone create, list, active, status, and completion behavior.
4. Slice create, read, update, status, and native blocking relations.
5. Task create, read, update, and status as sub-issues.
6. Standalone issue create, list, get, and status.
7. Milestone artifacts through Linear Documents.
8. Slice, task, and standalone issue artifacts through marker comments.
9. Snapshot reads with active milestone, roadmap documents, slices, tasks, artifacts, dependencies, implementation waves, verification evidence, and `nextAction`.
10. Negative tests for missing states, bad dependency IDs, wrong entity classification, and unavailable capabilities.
11. GitHub Projects v2 regression tests for the same domain contract.

Validation sequence:

1. Targeted Linear Vitest suites.
2. Existing GitHub Projects v2 adapter and snapshot suites.
3. `pnpm --filter @kata-sh/cli test`.
4. `pnpm run validate:affected` when implementation touches shared packages or generated skill bundles.

## Requirement Coverage

This spec covers the CLI portion of:

1. LIN-01 through LIN-10.
2. SKL-01 through SKL-04 as backend contract support for skills.
3. DEP-01 through DEP-03 and DEP-05 for CLI planning, snapshots, and execution selection.

This spec prepares later work for:

1. SYM-01 through SYM-04.
2. DEP-04.
3. PRL-01 through PRL-05.

## Symphony Handoff

Symphony remains configured by `WORKFLOW.md`.

Later Symphony work must:

1. Re-evaluate existing Symphony Linear support against this CLI contract.
2. Use CLI-backed Kata operations for durable Kata project, milestone, slice, task, artifact, verification, and progress effects where applicable.
3. Keep Symphony tracker configuration independent from `.kata/preferences.md`.
4. Preserve GitHub Projects v2 support.
5. Use the CLI snapshot or an equivalent shared dependency view before selecting dependency-gated work.

## PR/Land Handoff

PR/land validation is a separate GitHub workflow validation slice.

Later PR/land work must provide:

1. A representative PR validation fixture.
2. Review and comment handling validation on an open PR.
3. PR update and push validation through normal safety gates.
4. Check and merge-readiness validation.
5. Merge execution guarded by a disposable PR or explicit user approval.
