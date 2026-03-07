# Build-Time Harness Evidence Contract Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement an evidence-backed build-time harness that rejects metadata-only documentation compliance and requires traceable links between code changes, context loaded, verification evidence, and canonical-doc updates.

**Architecture:** Extend the existing `scripts/harness` contract from presence-based checks to relevance-based checks, add a new evidence artifact and context-routing manifest, and wire the policy into local git hooks plus repo-specific skill guidance. Start with the highest-risk surfaces so the harness improves behavior without forcing full paperwork on trivial changes.

**Tech Stack:** Bash, Node 22, TypeScript, pnpm, git hooks, markdown repo docs

---

Related skills during execution: `@executing-plans`, `@test-driven-development`, `@verification-before-completion`, `@writing-skills`.

### Task 1: Define the evidence artifact schema and context-routing contract

**Files:**
- Create: `docs/harness/context-map.yaml`
- Create: `docs/harness/change-evidence-schema.md`
- Modify: `docs/harness/BUILDING-WITH-HARNESS.md`
- Test: `scripts/harness/check_repo_contract.sh`

**Step 1: Write the failing contract check expectations**

Document the minimum required fields for:

- `docs/generated/change-evidence/<date>-<topic>.md`
- `docs/generated/change-evidence/<date>-<topic>.json`
- `docs/harness/context-map.yaml`

Include path-to-doc ownership examples for:

- `src/config/**`
- `src/execution/**`
- `src/orchestrator/**`
- `WORKFLOW.md`

**Step 2: Add the new docs to the repo contract**

Update `scripts/harness/check_repo_contract.sh` so it requires:

- `docs/harness/context-map.yaml`
- `docs/harness/change-evidence-schema.md`
- `docs/generated/change-evidence/` directory

Expected result:

- the repo contract fails until the new harness artifacts exist.

**Step 3: Run the repo contract check**

Run: `bash scripts/harness/check_repo_contract.sh`
Expected: FAIL before the new files exist, PASS after they are added.

**Step 4: Commit**

```bash
git add docs/harness/context-map.yaml docs/harness/change-evidence-schema.md docs/harness/BUILDING-WITH-HARNESS.md scripts/harness/check_repo_contract.sh
git commit -m "docs(harness): define evidence contract artifacts"
```

### Task 2: Add a machine-readable evidence generator and stub flow

**Files:**
- Create: `scripts/harness/generate_change_evidence.ts`
- Create: `docs/generated/change-evidence/.gitkeep`
- Create: `tests/harness/generate-change-evidence.test.ts`
- Modify: `package.json`

**Step 1: Write the failing test**

Create a harness test that feeds a synthetic changed-file list into the generator and expects:

- a JSON manifest with changed paths
- inferred impacted areas
- required canonical docs
- empty waiver slots
- placeholder verification fields

**Step 2: Implement the minimal generator**

Implement a TypeScript script that:

- reads changed files from `git diff --name-only` or an injected fixture input
- maps paths to impacted areas and required docs
- emits a JSON stub and markdown summary

Keep the first version deterministic and small.

**Step 3: Expose a package script**

Add a script such as:

- `harness:generate-evidence`

**Step 4: Run the targeted test**

Run: `pnpm vitest tests/harness/generate-change-evidence.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add scripts/harness/generate_change_evidence.ts tests/harness/generate-change-evidence.test.ts package.json docs/generated/change-evidence/.gitkeep
git commit -m "feat(harness): generate change evidence stubs"
```

### Task 3: Implement metadata-only and doc-relevance checks

**Files:**
- Create: `scripts/harness/check_doc_relevance.sh`
- Create: `tests/harness/check-doc-relevance.test.ts`
- Modify: `scripts/ci-local.sh`

**Step 1: Write the failing tests**

Cover at least these cases:

- canonical doc changed only in `Last reviewed`
- canonical doc changed only in whitespace
- canonical doc changed with substantive body changes
- unrelated canonical doc touched to satisfy docs-sync

**Step 2: Implement the minimal shell check**

The script should fail when:

- a canonical doc diff is metadata-only, or
- a canonical doc update is outside the required doc set inferred from the evidence artifact without an explicit waiver.

**Step 3: Wire the check into local CI**

Add the new script to `scripts/ci-local.sh`.

**Step 4: Run the targeted test and local CI script**

Run:

- `pnpm vitest tests/harness/check-doc-relevance.test.ts`
- `bash scripts/ci-local.sh`

Expected: PASS.

**Step 5: Commit**

```bash
git add scripts/harness/check_doc_relevance.sh tests/harness/check-doc-relevance.test.ts scripts/ci-local.sh
git commit -m "fix(harness): reject metadata-only doc compliance"
```

### Task 4: Implement the evidence-contract and decision-link checks

**Files:**
- Create: `scripts/harness/check_evidence_contract.sh`
- Create: `scripts/harness/check_decision_links.sh`
- Create: `tests/harness/check-evidence-contract.test.ts`
- Modify: `scripts/ci-local.sh`

**Step 1: Write the failing tests**

Cover:

- qualifying code changes with no evidence manifest
- evidence manifest missing required fields
- architecture-sensitive changes with no linked design or plan
- verification artifact missing command evidence
- explicit no-doc waiver with rationale

**Step 2: Implement the checks**

`check_evidence_contract.sh` should validate:

- required artifact presence above configured thresholds
- required JSON keys
- allowed waiver structure
- required context declaration fields

`check_decision_links.sh` should validate:

- links between evidence artifact and plan/design docs
- links between evidence artifact and verification artifacts
- links between impacted subsystems and updated canonical docs

**Step 3: Wire the checks into local CI**

Add both scripts to `scripts/ci-local.sh`.

**Step 4: Run tests**

Run:

- `pnpm vitest tests/harness/check-evidence-contract.test.ts`
- `bash scripts/ci-local.sh`

Expected: PASS.

**Step 5: Commit**

```bash
git add scripts/harness/check_evidence_contract.sh scripts/harness/check_decision_links.sh tests/harness/check-evidence-contract.test.ts scripts/ci-local.sh
git commit -m "feat(harness): enforce evidence-backed repo contract"
```

### Task 5: Add pre-commit and strengthen pre-push behavior

**Files:**
- Create: `.githooks/pre-commit`
- Modify: `.githooks/pre-push`
- Modify: `scripts/install-githooks.sh`
- Test: `.githooks/pre-commit`

**Step 1: Write the pre-commit behavior**

`pre-commit` should run fast, local checks only:

- `check_doc_relevance.sh`
- `check_evidence_contract.sh` in staged-file mode

**Step 2: Keep pre-push as the full gate**

Ensure `pre-push` continues to run the full local CI path with the new harness checks included.

**Step 3: Verify hook installation path**

Run:

- `bash scripts/install-githooks.sh`
- staged dry-run verification for `pre-commit`

Expected:

- hooks install cleanly and run the new checks.

**Step 4: Commit**

```bash
git add .githooks/pre-commit .githooks/pre-push scripts/install-githooks.sh
git commit -m "chore(harness): add pre-commit evidence gates"
```

### Task 6: Align repo-specific skill guidance with the new contract

**Files:**
- Create: `.codex/skills/symphony-harness-evidence/SKILL.md`
- Modify: `AGENTS.md`
- Modify: `docs/harness/BUILDING-WITH-HARNESS.md`
- Test: manual skill invocation sanity check

**Step 1: Write the skill content**

Define:

- when evidence is required
- how to map changed files to required docs
- what counts as a valid no-doc waiver
- how to keep evidence artifacts current through execution

**Step 2: Register the skill**

Add it to `AGENTS.md` so agents discover it during work in this repo.

**Step 3: Validate the guidance against one existing plan or ticket flow**

Run a manual dry run using a recent harness-sensitive ticket and confirm the skill guidance matches the script-enforced rules.

**Step 4: Commit**

```bash
git add .codex/skills/symphony-harness-evidence/SKILL.md AGENTS.md docs/harness/BUILDING-WITH-HARNESS.md
git commit -m "docs(harness): add evidence-contract skill guidance"
```

### Task 7: Add periodic drift auditing for stale docs and orphaned decisions

**Files:**
- Create: `scripts/harness/check_stale_context.sh`
- Create: `tests/harness/check-stale-context.test.ts`
- Modify: `QUALITY_SCORE.md`
- Modify: `PLANS.md`

**Step 1: Write the failing tests**

Cover:

- subsystem paths changed repeatedly without owning-doc updates
- design docs with no linked implementation plan or verification trail
- evidence artifacts that reference docs no longer present

**Step 2: Implement the minimal drift checker**

The first version can inspect recent git history and current docs layout to flag obvious drift candidates.

**Step 3: Decide how to surface the audit**

Start by making it an opt-in CI/report script rather than a hard gate.

**Step 4: Run tests**

Run:

- `pnpm vitest tests/harness/check-stale-context.test.ts`
- `bash scripts/harness/check_stale_context.sh`

Expected: PASS on fixtures and produce a readable audit summary.

**Step 5: Commit**

```bash
git add scripts/harness/check_stale_context.sh tests/harness/check-stale-context.test.ts QUALITY_SCORE.md PLANS.md
git commit -m "feat(harness): add stale context drift audit"
```

### Task 8: Run full verification and capture rollout evidence

**Files:**
- Modify: `docs/generated/README.md`
- Create: `docs/generated/change-evidence/<date>-build-time-harness.md`
- Create: `docs/generated/change-evidence/<date>-build-time-harness.json`
- Create: `docs/generated/<ticket-or-topic>-verification.md`

**Step 1: Generate the evidence artifacts**

Run the evidence generator for the harness implementation diff and fill in:

- docs loaded
- docs updated
- commands run
- residual risks

**Step 2: Run full verification**

Run:

- `pnpm run lint`
- `pnpm run typecheck`
- `pnpm test`
- `bash scripts/ci-local.sh`
- `make check`

Expected: PASS.

**Step 3: Review the generated artifacts**

Confirm they contain substantive links and are not satisfiable with metadata-only content.

**Step 4: Commit**

```bash
git add docs/generated/README.md docs/generated/change-evidence docs/generated/*verification.md
git commit -m "test(harness): verify build-time evidence contract rollout"
```

## Execution Notes

- Keep the first rollout strict only for architecture-sensitive paths and canonical docs.
- Prefer shell checks for diff inspection and path ownership logic; use TypeScript where structured generation or parsing improves clarity.
- Keep evidence schema compact so agents do not optimize for paperwork volume.
- Use verification artifacts as an input to canonical doc freshness rather than a parallel documentation system.
