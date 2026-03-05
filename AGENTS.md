# Symphony Repository Agent Guide

This file is intentionally short. Treat it as a map, not an encyclopedia.

## Mission

Build and operate `Symphony` from `SPEC.md` with an agent-first workflow.

## Source Of Truth

1. Product contract: `SPEC.md`
2. Architecture map: `ARCHITECTURE.md`
3. Active plans: `PLANS.md`
4. Reliability policy: `RELIABILITY.md`
5. Security posture: `SECURITY.md`
6. Quality baseline: `QUALITY_SCORE.md`
7. Harness rules: `docs/references/harness-engineering.md`
8. Execution workflow contract: `WORKFLOW.md`

## Working Rules

1. Keep repository knowledge versioned in this repo.
2. Prefer small, reviewable changes over long-lived branches.
3. Add tests with behavior changes.
4. Update documentation when behavior changes.
5. Run harness checks before claiming completion:
   - `make check`

## Ticket Workflow

Use this flow for every Linear implementation ticket.

1. Pick ticket
   - Move ticket to `In Progress`.
   - Read scope, acceptance criteria, dependencies, and `gitBranchName`.
2. Create branch
   - Create/switch to the ticket branch using Linear `gitBranchName`.
   - Do not work on `main`.
3. Design first (required for feature/behavior changes)
   - Use `superpowers` + `brainstorming`.
   - Get design approval before implementation.
4. Plan implementation
   - Use planning workflow (`writing-plans` or repo planning docs).
   - Define tests first (TDD: red -> green -> refactor).
5. Implement
   - Keep changes scoped to ticket acceptance criteria.
   - Update docs alongside behavior changes.
6. Verify
   - Run `make check`.
   - Run ticket-specific tests and record evidence.
7. Open PR
   - Include: scope summary, changed files, test evidence, conformance notes, residual risks.
8. Review and fix
   - Address review comments.
   - Re-run checks after fixes.
9. Merge gate
   - CI required checks pass.
   - At least one approval.
   - No unresolved blocking comments.
10. Closeout
   - Merge PR.
   - Move Linear ticket to the next workflow state (`In Review`/`Done` per team policy).

## Local Symphony Skills

- `symphony-start-work`: Use when starting a Symphony Linear issue to map scope to `SPEC.md` and establish verification boundaries.
- `symphony-implement-core`: Use when implementing core service behavior with required harness gates and docs updates.
- `symphony-verify-conformance`: Use before review/merge to map evidence to `SPEC.md` Sections 17 and 18.
- `symphony-runtime-hardening`: Use when changing runtime safety defaults (approval/sandbox/hooks/logging/security posture).
- `symphony-dogfood`: Use for controlled “Symphony builds Symphony” execution cycles and outcome reporting.
- `symphony-ship-gate`: Use at release checkpoints to produce explicit go/no-go decisions with blocker/risk summaries.
- `symphony-tui-ops`: Use when building or validating the TUI monitoring surface and operator workflows.

## Documentation Layout

- `docs/design-docs/`: technical design records
- `docs/exec-plans/active/`: current execution plans
- `docs/exec-plans/completed/`: archived execution plans
- `docs/generated/`: generated reference material
- `docs/product-specs/`: product/spec indexes
- `docs/references/`: external references and adapted guidance
