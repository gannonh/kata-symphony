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
9. Linear Project: `https://linear.app/kata-sh/project/symphony-service-v1-spec-execution-5f0020cb5273`

## Working Rules

1. Keep repository knowledge versioned in this repo.
2. Prefer small, reviewable changes over long-lived branches.
3. Add tests with behavior changes.
4. Update documentation when behavior changes.
5. Run harness checks before claiming completion:
   - `make check`
6. Never bypass git hooks with `--no-verify`.
   - If pre-push fails, fix the underlying issue (including coverage thresholds) and rerun normally.

## Workflow

Use this single workflow in this repo.

1. Start ticket lifecycle
   - `kata-linear start KAT-<number>`

2. Design
   - `superpowers:brainstorming`

3. Plan
   - `superpowers:writing-plans`

4. Execute implementation
   - `superpowers:subagent-driven-development`
   - `superpowers:test-driven-development`
   - `superpowers:requesting-code-review`

5. Verify before claiming completion
   - `superpowers:verification-before-completion`
   - `user-acceptance`

6. Close ticket lifecycle
   - `superpowers:finishing-a-development-branch`
   - `kata-linear end KAT-<number>`
  
7. PR workflow
   - `pull-requests` create PR
   - `gh-address-comments` for PR comments
   - `gh-fix-ci` for CI failures
   - `pr-review-plugin:pr-review` for PR reviews
   - `pull-requests` merge pr when approved and CI is green

## Optional Domain Skills

Use these only when needed for domain-specific work.
They do not replace `kata-linear`, `brainstorming`, `writing-plans`, or `executing-plans`.

1. Runtime safety posture changes
   - `symphony-runtime-hardening`
2. TUI monitoring surface work
   - `symphony-tui-ops`
3. Symphony-on-Symphony dogfood runs
   - `symphony-dogfood`
4. Release go/no-go checkpoints
   - `symphony-ship-gate`
5. Evidence-backed harness changes
   - `symphony-harness-evidence`

## Documentation Layout

- `docs/design-docs/`: technical design records
- `docs/exec-plans/active/`: current execution plans
- `docs/exec-plans/completed/`: archived execution plans
- `docs/generated/`: generated reference material
- `docs/product-specs/`: product/spec indexes
- `docs/references/`: external references and adapted guidance
- `docs/harness/`: repo harness contracts, context maps, and evidence rules
