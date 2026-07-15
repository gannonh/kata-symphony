---
type: Spec
title: Symphony Linear Execution Design
description: Archived spec document: Symphony Linear Execution Design.
tags: [archive, symphony]
timestamp: 2026-07-15T20:00:00Z
---

# Symphony Linear Execution Design

**Date:** 2026-05-06
**Status:** Superseded by `docs/superpowers/specs/2026-05-07-symphony-linear-execution-and-backend-uat-design.md`
**Scope:** Update Symphony so Linear-backed execution uses the same backend abstraction and dependency-aware dispatch behavior as GitHub Projects v2.

This file is retained as historical context. Use the superseding spec for current implementation work.

## Problem

Symphony already polls trackers directly and dispatches agent workers from normalized tracker issue state. GitHub Projects v2 support includes status-based candidate selection and dependency-aware dispatch. Linear support exists, but it needs to align with the same backend abstraction and with the Linear backend model defined in `2026-05-06-cli-linear-core-design.md`.

This spec covers Symphony execution behavior. CLI Linear backend implementation remains in the CLI Linear Core spec. PR/land validation remains a separate GitHub workflow validation slice.

## Goals

1. Symphony can execute Kata issues, planned slices, and slice tasks backed by Linear.
2. Symphony keeps `WORKFLOW.md` as its configuration source.
3. Symphony preserves GitHub Projects v2 support.
4. Symphony dispatch selection stays inside backend-specific tracker adapters behind the shared `TrackerAdapter` boundary.
5. Linear dispatch observes native Linear workflow states, parent/sub-issues, project milestones, and blocking relations.
6. Worker skills use backend-neutral Symphony helper operations for tracker state and Kata CLI operations for durable Kata workflow effects when applicable.

## Non-goals

1. Implementing CLI Linear Core.
2. Reading `.kata/preferences.md` from Symphony.
3. Replacing Symphony dispatch selection with `kata project.getSnapshot`.
4. Live Linear API validation against a production workspace.
5. PR/land validation fixture work.

## Architecture

Symphony keeps its direct tracker-polling model.

`WORKFLOW.md` selects the tracker backend. Symphony builds a backend adapter that implements `TrackerAdapter`.

The tracker boundary normalizes each backend into `Issue` records with:

1. opaque backend ID
2. display identifier
3. title and description
4. workflow state
5. labels
6. branch name and URL
7. assignee routing result
8. parent issue identifier
9. child count
10. blocker references with ID, identifier, and state when available

GitHub Projects v2 remains the reference backend for this shape. Linear support returns the same normalized dispatch data from Linear Project, Project Milestone, issue, sub-issue, workflow state, and blocking relation primitives.

Kata CLI is used by worker skills for durable Kata workflow effects when a worker executes Kata work. Symphony dispatch selection stays inside Symphony tracker adapters.

## Data Flow

Startup and polling:

1. Symphony reads `WORKFLOW.md`.
2. It builds a `GithubAdapter`, `LinearAdapter`, or future backend adapter from `tracker.kind`.
3. Each tick runs reconcile, validate, then dispatch fetch.
4. `fetch_candidate_issues` returns active-state issues normalized into the shared `Issue` shape.
5. Symphony sorts candidates, collects dependency-blocked candidates for visibility, then dispatches eligible parent issues until concurrency slots are full.

Linear dispatch model:

1. Project comes from `tracker.project_slug` or equivalent config.
2. Project milestone filters candidate work when configured.
3. Slice issues are dispatch candidates.
4. Task sub-issues are not dispatched independently.
5. Standalone execution issues can dispatch when they match active states and are not excluded.
6. Linear blocking relations populate `Issue.blocked_by`.

Worker state flow:

1. Symphony helper operations continue using `sym-state`.
2. Tracker transitions and comments use the active Symphony tracker adapter.
3. Kata workflow state, artifacts, verification evidence, and planning or execution effects use Kata CLI operations from skills when the worker is doing Kata-backed work.

## Dependencies And Status

Symphony keeps current dependency semantics:

1. Known blockers in non-terminal states block dispatch.
2. Known blockers in terminal states do not block dispatch.
3. Blockers with unresolved or inaccessible state log a warning and do not block dispatch.
4. Circular dependencies are logged for operator visibility.
5. Blocked issues appear in the TUI and dashboard blocked list.

Status semantics come from `WORKFLOW.md`:

1. `active_states` define dispatch candidates.
2. `terminal_states` define completed blockers and completed running work.
3. Backend adapters map native workflow state into the configured names.
4. Default states remain the current Kata defaults. Configured values are authoritative.

For Kata-shaped work, Symphony dispatches the parent slice issue and skips task sub-issues as independent workers. Existing `exclude_labels` behavior remains available.

## Helper And Skills

`sym-state` stays the backend-neutral worker helper. It supports the same core operations for GitHub Projects v2 and Linear:

1. `issue.get`
2. `issue.list-children`
3. `comment.upsert`
4. `issue.update-state`
5. `issue.create-followup`
6. `document.read`
7. `document.write`

Linear helper support is updated to parity for issue details, child issues, comments, and Symphony helper/workpad documents.

PR helpers remain GitHub-only:

1. `pr.inspect-feedback`
2. `pr.inspect-checks`
3. `pr.land-status`

`sym-linear` remains a manual fallback for explicit Linear maintenance. Normal worker tracker state flows through `sym-state`. Kata durable workflow effects flow through Kata CLI operations when applicable.

## Error Handling

1. Invalid `WORKFLOW.md` tracker config blocks dispatch during validation.
2. Missing backend auth blocks dispatch with an actionable doctor or config error.
3. Missing Linear project, team, project milestone, or state metadata blocks dispatch.
4. Missing child issue, comment, document, or relation capability blocks only operations that require that capability.
5. Helper failures return `{"ok": false, "error": {"message": "<operation-specific message>"}}` and workers record the error before deciding whether it is blocking.
6. Backend adapter errors include backend kind and operation context.

## Testing

Required coverage:

1. Linear adapter tests for candidate fetch, active-state filtering, project filtering, milestone filtering, sub-issue normalization, blocker normalization, assignee routing, and state updates.
2. Orchestrator tests proving Linear blockers gate dispatch with the same semantics as GitHub.
3. Helper tests for Linear `issue.get`, `issue.list-children`, `comment.upsert`, `document.read`, `document.write`, and `issue.create-followup`.
4. Existing GitHub Projects v2 tests continue to pass.
5. Backend-neutral worker contract tests prevent Symphony skills from using backend-specific tracker operations for normal state flow.

Validation sequence:

1. Targeted Symphony Rust tests for Linear client and adapter behavior.
2. Targeted orchestrator dispatch tests.
3. Targeted helper tests.
4. `cargo test` in `apps/symphony`.
5. `pnpm run validate:affected` when changes touch generated skills or shared packages.

## Requirement Coverage

This spec covers:

1. SYM-01 through SYM-04.
2. DEP-04.
3. The Symphony-facing parts of SKL-02 and SKL-03.

This spec depends on:

1. CLI Linear Core for durable Kata backend operations.
2. Existing GitHub Projects v2 behavior for regression parity.

## Handoff To Implementation Planning

Implementation planning splits work into these units:

1. Linear tracker adapter normalization.
2. Dependency-aware dispatch tests and any required adapter fixes.
3. Linear helper parity.
4. Symphony skill contract updates and regression tests.
5. GitHub Projects v2 regression validation.
