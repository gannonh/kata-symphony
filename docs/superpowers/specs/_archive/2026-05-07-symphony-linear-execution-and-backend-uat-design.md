---
type: Spec
title: Symphony Linear Execution And Backend Uat Design
description: "Archived spec document: Symphony Linear Execution And Backend Uat Design."
tags: [archive, symphony]
timestamp: 2026-07-15T20:00:00Z
---

# Symphony Linear Execution And Backend UAT Design

**Date:** 2026-05-07
**Status:** Draft for review
**Scope:** Update Symphony Linear execution and add a real-backend helper UAT skill for GitHub Projects v2 and Linear.

## Problem

Symphony already executes workers from tracker state read through `WORKFLOW.md`. GitHub Projects v2 includes status-based candidate selection and dependency-aware dispatch. Linear has foundation for polling and state updates, and the direct helper surface needs parity with GitHub for normal worker state flow.

The prior Symphony Linear plan referenced distributed Symphony skills. Current Symphony worker guidance lives in prompts, and workers call the binary directly through `$SYMPHONY_BIN helper <operation>`.

The CLI Linear Core work is complete on this branch. It established GitHub and Linear backend parity for 22 Kata CLI operations, compact artifact comment markers, marker-free Linear project and milestone documents, closed GitHub item handling in health checks, and a `kata-backend-uat` skill for real backend proof runs.

## Goals

1. Symphony executes Linear-backed work through the existing `TrackerAdapter` boundary.
2. Linear candidate polling observes configured active states, terminal states, project scope, parent/sub-issue shape, assignee routing, and native blocking relations.
3. Direct helper operations work against GitHub Projects v2 and Linear for normal worker tracker flow.
4. GitHub-only PR helper operations continue to run for GitHub-backed workflows.
5. Worker prompts describe the direct helper contract through `$SYMPHONY_BIN helper`.
6. A sibling `.agents/skills/symphony-backend-uat` skill proves the direct Symphony helper surface against real GitHub and Linear backends.
7. UAT evidence includes provider links for generated comments, documents, and issues.
8. Generated UAT contracts update from Symphony source and prompts.

## Non-Goals

1. Replacing Symphony tracker polling with Kata CLI snapshot reads.
2. Reading `.kata/preferences.md` from Symphony.
3. Running a live worker dispatch cycle in the first UAT skill.
4. Implementing CLI backend behavior already covered by CLI Linear Core.
5. Reintroducing distributed Symphony skills.

## Architecture

Symphony keeps `WORKFLOW.md` as its execution configuration source. `tracker.kind` selects the backend adapter. The orchestrator continues to use normalized `Issue` records returned by `TrackerAdapter`.

Linear support expands the existing `LinearClient` and `LinearAdapter` so GitHub and Linear produce the same dispatch fields where each backend has matching concepts:

1. opaque issue ID
2. display identifier
3. title and description
4. workflow state
5. labels
6. branch name and URL
7. assignee routing result
8. parent issue identifier
9. child count
10. blocker references with ID, identifier, and state when available

Helper execution moves toward a testable boundary, such as `apps/symphony/src/helper.rs`, with `apps/symphony/src/main.rs` responsible for CLI argument handling and JSON output. The helper module keeps operation names backend-neutral and routes through the configured tracker backend.

The UAT skill mirrors the shape of `.agents/skills/kata-backend-uat`:

1. `test --backend github`
2. `test --backend linear`
3. `update`
4. `cleanup --evidence <path>`

`symphony-backend-uat` builds the local Symphony binary when needed, writes isolated `WORKFLOW.md` fixtures, calls the binary helper subcommand, verifies provider state directly, writes evidence, and cleans up created test records.

## Direct Helper Contract

Shared helper operations:

1. `issue.get`
2. `issue.list-children`
3. `comment.upsert`
4. `issue.update-state`
5. `issue.create-followup`
6. `document.read`
7. `document.write`

GitHub-only helper operations:

1. `pr.inspect-feedback`
2. `pr.inspect-checks`
3. `pr.land-status`

Shared helper responses include enough provider data for UAT proof links. Comments and documents created by helpers include stable identifiers or URLs where the provider exposes them.

## Linear Execution Behavior

Candidate polling uses Linear issue queries scoped to the configured project and issue states. Symphony carries tracker issues through their lifecycle regardless of how those issues were created.

Dispatch rules:

1. Parent issues can dispatch when active and unblocked.
2. Sub-issues provide child context for the parent worker.
3. Standalone issues can dispatch when active and unblocked.
4. Issues with excluded labels stay out of candidate results.
5. Native Linear blocking relations populate `Issue.blocked_by`.
6. Non-terminal known blockers gate dispatch.
7. Terminal blockers allow dispatch.
8. Blockers with unavailable state are reported for visibility and do not gate dispatch.

## Prompt Contract

Worker prompts direct tracker reads, tracker comments, tracker state changes, helper documents, and PR inspection through:

```bash
"$SYMPHONY_BIN" helper <operation> --workflow "$SYMPHONY_WORKFLOW_PATH" --input "$INPUT"
```

Prompts describe helper payloads and temporary input file handling. Prompt tests guard against backend-specific tracker commands in worker prompts.

Kata workflow artifacts, planning effects, verification evidence, and CLI backend operations remain Kata CLI responsibilities when workers are performing Kata work.

## Symphony Backend UAT Skill

Create `.agents/skills/symphony-backend-uat` with:

1. `SKILL.md`
2. `scripts/symphony-backend-uat.mjs`
3. `references/backend-config.md`
4. `references/evidence.md`
5. `references/generated-symphony-contract.json`
6. `references/self-update.md`
7. `references/workflow.md`
8. `evals/evals.json`

The skill asks which action to run unless the user already specified it:

1. Test a backend
2. Update this skill from Symphony changes
3. Clean up a prior test run

The `update` command parses Symphony source and prompts to refresh:

1. shared helper operation names
2. GitHub-only helper operation names
3. supported tracker backend kinds
4. prompt files that mention helper usage
5. Symphony package or git metadata

The `test` command:

1. creates an isolated run directory
2. writes a backend-specific `WORKFLOW.md`
3. builds or locates the Symphony binary
4. runs doctor or equivalent health checks
5. creates a real parent issue or identifies a supplied one
6. runs every shared helper operation against the selected backend
7. runs GitHub-only PR helper operations when a PR is discoverable
8. fetches created provider records back through GitHub REST or Linear GraphQL
9. writes `evidence.json`, `evidence.md`, and helper payload files
10. completes or closes created test records during cleanup

The `cleanup` command reads the evidence file and completes or closes created records. Cleanup success is reported separately from test success.

## UAT Pass Criteria

A backend helper test passes when:

1. health checks pass
2. every shared helper operation is observed
3. provider records created by helpers are fetched back from the provider
4. proof links are present for comments, documents, and follow-up issues when the provider exposes URLs
5. GitHub PR helpers pass when a PR is discoverable, or are reported as skipped with the reason
6. evidence files are written
7. cleanup status is recorded

## Error Handling

1. Invalid `WORKFLOW.md` config returns a helper error with the config field name.
2. Missing auth returns a helper error naming the expected environment variable or config field.
3. Missing Linear project, state, comment, child issue, document, or relation data returns operation-specific errors.
4. Provider rate limits and transient 5xx failures are retried by the UAT runner with retry counts in evidence.
5. Unsupported helper operations return structured helper errors.
6. GitHub-only PR helpers return a GitHub-only error for Linear-backed workflows.

## Testing

Required automated coverage:

1. config regression tests covering the existing Linear tracker fields
2. Linear client tests for candidate queries, child issues, comments, documents, and follow-up issue creation
3. Linear adapter tests for normalized candidate and state behavior
4. orchestrator tests for Linear dependency gates
5. helper contract tests for GitHub and Linear helper routing
6. prompt contract tests for direct `$SYMPHONY_BIN helper` usage
7. GitHub Projects v2 regression tests
8. `symphony-backend-uat` dry-run and contract update tests

Validation sequence:

1. targeted Symphony Rust tests for changed modules
2. full `cargo test --manifest-path apps/symphony/Cargo.toml`
3. `pnpm run validate:affected`
4. `node .agents/skills/symphony-backend-uat/scripts/symphony-backend-uat.mjs update`
5. `node .agents/skills/symphony-backend-uat/scripts/symphony-backend-uat.mjs test --backend github`
6. `node .agents/skills/symphony-backend-uat/scripts/symphony-backend-uat.mjs test --backend linear`

Live UAT commands run only when the user requests real backend proof.

## Requirement Coverage

This spec covers:

1. Linear tracker adapter normalization
2. Linear dependency-aware dispatch
3. direct helper parity across GitHub and Linear
4. GitHub-only PR helper validation
5. prompt contract updates
6. real backend helper proof through `symphony-backend-uat`
7. generated contract self-update for Symphony helper changes

This spec depends on:

1. completed CLI Linear Core behavior
2. existing GitHub Projects v2 Symphony adapter behavior
3. existing `kata-backend-uat` structure as the sibling skill reference

## Handoff To Implementation Planning

Implementation planning splits work into these units:

1. mark stale Symphony spec and plan as superseded
2. normalize Linear candidate issue data for dispatch parity
3. prove Linear child-issue and dependency gates in orchestrator tests
4. implement Linear helper parity behind a testable helper boundary
5. update worker prompt contracts
6. create `symphony-backend-uat`
7. validate GitHub and Linear regressions
