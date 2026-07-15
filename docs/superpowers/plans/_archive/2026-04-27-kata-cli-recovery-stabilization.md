---
type: Plan
title: Kata Cli Recovery Stabilization
description: Archived implementation plan: Kata Cli Recovery Stabilization.
tags: [archive]
timestamp: 2026-07-15T20:00:00Z
---

# Kata CLI Recovery and Stabilization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore confidence by shipping one fully validated golden path (`Pi + Skills + @kata-sh/cli + GitHub Projects v2`) before expanding scope.

**Architecture:** Keep skills uniform and harness-agnostic, route all backend IO through the typed `@kata-sh/cli` domain API, and keep harness differences in thin installer/plugin adapters. Desktop remains the integrated distribution and uses Pi directly in RPC mode while reusing the same CLI/backend contract.

**Tech Stack:** TypeScript, Node.js, Bun/Vitest, pnpm, Electron, GitHub Actions, Kata Skills spec

**Spec:** `/Users/gannonhall/.codex/worktrees/edf7/kata-mono/docs/superpowers/specs/2026-04-27-kata-cli-recovery-stabilization-design.md`

---

## Scope Guardrails

1. No new harness feature expansion before golden path is green.
2. No label-mode fallback in any runtime/docs/types.
3. No backend-specific branching in skill logic or harness adapters.
4. No “done” claim without both automated and manual validation evidence.

## File Map

### Create

| File | Responsibility |
| --- | --- |
| `docs/superpowers/specs/2026-04-27-kata-cli-capability-matrix.md` | Canonical mapping: workflow -> skill -> runtime op -> test evidence |
| `apps/cli/src/tests/golden-path.pi-github.vitest.test.ts` | Golden-path runtime smoke tests (Pi env + GitHub contract surface) |
| `apps/orchestrator/tests/skill-coverage-matrix.test.js` | Enforces manifest/workflow/skill mapping invariants |
| `docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md` | Manual QA steps with expected outcomes |

### Modify

| File | Change |
| --- | --- |
| `docs/superpowers/specs/2026-04-27-kata-cli-skill-platform-gap-assessment.md` | Replace ambiguous migration language with matrix-linked status |
| `apps/orchestrator/skills-src/manifest.json` | Expand/normalize emitted core skills for golden path |
| `apps/orchestrator/scripts/build-skill-bundle.js` | Embed strict references to CLI contract operations and skill metadata completeness checks |
| `apps/orchestrator/tests/build-skill-bundle.test.js` | Assert generated skills include expected workflow references |
| `apps/cli/src/transports/json.ts` | Replace stub backend wiring with live adapter clients required for golden path ops |
| `apps/cli/src/commands/setup.ts` | Implement real `setup --pi` installer and verification |
| `apps/cli/src/commands/doctor.ts` | Report actionable install/config/runtime health checks |
| `apps/cli/src/backends/read-tracker-config.ts` | Enforce Projects v2-only policy and strict errors |
| `apps/cli/src/resources/extensions/kata/github-config.ts` | Remove label-mode schema branches |
| `apps/cli/src/resources/extensions/kata/docs/preferences-reference.md` | Remove label-mode documentation and examples |
| `apps/desktop/src/main/workflow-config-reader.ts` | Remove label-mode parsing/typing and align with CLI policy |
| `apps/desktop/src/shared/types.ts` | Eliminate label-mode enum/state options |
| `scripts/ci/build-kata-distributions.sh` | Add golden-path verification hooks (not only artifact checks) |
| `.github/workflows/ci.yml` | Add/require golden-path smoke lane |

---

### Task 1: Establish migration truth and control documents

**Files:**
- Create: `docs/superpowers/specs/2026-04-27-kata-cli-capability-matrix.md`
- Modify: `docs/superpowers/specs/2026-04-27-kata-cli-skill-platform-gap-assessment.md`

- [ ] **Step 1: Build the baseline matrix**

Capture every workflow under `apps/orchestrator/kata/workflows/` and map it to:
- legacy command (if any)
- emitted skill (if any)
- runtime contract operation(s)
- status: `core`, `internal-only`, `consolidated`, `pending`

- [ ] **Step 2: Define “migrated” criteria in the matrix header**

Use one explicit rule:
`migrated = skill surfaced + runtime contract path implemented + validation evidence recorded`

- [ ] **Step 3: Link matrix into gap-assessment doc**

Replace any “conceptual existence” language with matrix-backed status references.

- [ ] **Step 4: Validate documentation integrity**

Run: `rg -n "source of truth|conceptually in the new system|canonical workflow" docs/superpowers/specs/2026-04-27-kata-cli-skill-platform-gap-assessment.md`

Expected: legacy-overstating phrases removed or explicitly qualified.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-04-27-kata-cli-capability-matrix.md docs/superpowers/specs/2026-04-27-kata-cli-skill-platform-gap-assessment.md
git commit -m "docs: add capability matrix and migration truth criteria"
```

### Task 2: Lock Projects v2-only policy across code and docs

**Files:**
- Modify: `apps/cli/src/backends/read-tracker-config.ts`
- Modify: `apps/cli/src/resources/extensions/kata/github-config.ts`
- Modify: `apps/cli/src/resources/extensions/kata/docs/preferences-reference.md`
- Modify: `apps/desktop/src/main/workflow-config-reader.ts`
- Modify: `apps/desktop/src/shared/types.ts`

- [ ] **Step 1: Write/extend failing tests for label-mode rejection**

Target files:
- `apps/cli/src/tests/domain/adapters.vitest.test.ts`
- `apps/desktop/src/main/__tests__/workflow-config-reader.test.ts`

Add cases asserting label mode fails with explicit error text and remediation.

- [ ] **Step 2: Remove label mode from parsers/types**

Delete label-mode enum/value branches and fallback transforms in CLI + Desktop config readers.

- [ ] **Step 3: Remove label mode from docs**

Update preferences/config docs to show Projects v2 as the only GitHub mode.

- [ ] **Step 4: Run tests**

Run:
- `pnpm --dir apps/cli run test:vitest`
- `pnpm --dir apps/desktop run test`

Expected: all config parsing tests pass; no references to label mode remain in tested paths.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/backends/read-tracker-config.ts apps/cli/src/resources/extensions/kata/github-config.ts apps/cli/src/resources/extensions/kata/docs/preferences-reference.md apps/desktop/src/main/workflow-config-reader.ts apps/desktop/src/shared/types.ts apps/cli/src/tests/domain/adapters.vitest.test.ts apps/desktop/src/main/__tests__/workflow-config-reader.test.ts
git commit -m "refactor: remove github label mode and enforce projects v2 only"
```

### Task 3: Complete standalone runtime backend wiring for golden-path operations

**Files:**
- Modify: `apps/cli/src/transports/json.ts`
- Modify: `apps/cli/src/backends/resolve-backend.ts`
- Modify: `apps/cli/src/index.ts`
- Test: `apps/cli/src/tests/domain/service.vitest.test.ts`
- Test: `apps/cli/src/tests/domain/adapters.vitest.test.ts`

- [ ] **Step 1: Enumerate golden-path operations**

Define minimum required contract ops for Pi skills:
- project context read
- active milestone read
- slice/task list
- artifact read/write
- execution status read

- [ ] **Step 2: Replace stub client behavior in runtime path**

Ensure runtime transport uses real adapter clients for GitHub Projects v2.

- [ ] **Step 3: Add contract assertions**

Tests must fail if returned object shapes diverge by backend for the same operation.

- [ ] **Step 4: Run CLI tests**

Run:
- `pnpm --dir apps/cli run typecheck`
- `pnpm --dir apps/cli run test:vitest`

Expected: green tests; no stub placeholder responses in golden-path transport.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/transports/json.ts apps/cli/src/backends/resolve-backend.ts apps/cli/src/index.ts apps/cli/src/tests/domain/service.vitest.test.ts apps/cli/src/tests/domain/adapters.vitest.test.ts
git commit -m "feat(cli): wire live github adapter through standalone transport"
```

### Task 4: Implement real Pi setup/doctor flow

**Files:**
- Modify: `apps/cli/src/commands/setup.ts`
- Modify: `apps/cli/src/commands/doctor.ts`
- Test: `apps/cli/src/tests/setup.vitest.test.ts`

- [ ] **Step 1: Write failing setup tests**

Add tests for:
- `setup --pi` installs skills into expected Pi-visible location
- setup mutates or creates required Pi config/hook files
- setup emits machine-readable success/failure diagnostics

- [ ] **Step 2: Implement installer behavior**

Implement filesystem/config operations required for Pi integration; keep idempotent behavior on repeated runs.

- [ ] **Step 3: Harden doctor checks**

Doctor should validate:
- CLI binary availability
- skill bundle presence
- Pi integration files/hooks presence
- tracker/backend config parseability

- [ ] **Step 4: Run setup tests**

Run: `pnpm --dir apps/cli run test:vitest`

Expected: setup/doctor tests pass with clear diagnostics.

- [ ] **Step 5: Commit**

```bash
git add apps/cli/src/commands/setup.ts apps/cli/src/commands/doctor.ts apps/cli/src/tests/setup.vitest.test.ts
git commit -m "feat(cli): implement setup --pi installer and doctor health checks"
```

### Task 5: Expand and validate core skill surface

**Files:**
- Modify: `apps/orchestrator/skills-src/manifest.json`
- Modify: `apps/orchestrator/scripts/build-skill-bundle.js`
- Modify: `apps/orchestrator/tests/build-skill-bundle.test.js`
- Create: `apps/orchestrator/tests/skill-coverage-matrix.test.js`

- [ ] **Step 1: Define core skill set for golden path**

Core list:
- `new-project`
- `discuss-phase`
- `plan-phase`
- `execute-phase`
- `verify-work`
- `quick`
- `progress`
- `health`

- [ ] **Step 2: Update manifest and generated metadata**

Ensure each emitted skill includes canonical workflow reference and required setup hints.

- [ ] **Step 3: Add coverage test**

Fail build when a core workflow lacks emitted skill mapping or is marked undocumented.

- [ ] **Step 4: Build and test**

Run:
- `pnpm --dir apps/orchestrator run build:skills`
- `pnpm --dir apps/orchestrator run test`

Expected: emitted skills include full core list and tests enforce invariants.

- [ ] **Step 5: Commit**

```bash
git add apps/orchestrator/skills-src/manifest.json apps/orchestrator/scripts/build-skill-bundle.js apps/orchestrator/tests/build-skill-bundle.test.js apps/orchestrator/tests/skill-coverage-matrix.test.js
git commit -m "feat(orchestrator): enforce core skill coverage for golden path"
```

### Task 6: Add golden-path smoke tests and runbook

**Files:**
- Create: `apps/cli/src/tests/golden-path.pi-github.vitest.test.ts`
- Create: `docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md`

- [ ] **Step 1: Add automated smoke test**

Test should execute:
1. Pi harness detection path
2. setup/doctor command flow
3. at least one GitHub-backed domain operation through runtime transport

- [ ] **Step 2: Write manual runbook**

Include exact commands, required env vars, and expected pass/fail outputs for:
- local Pi setup
- skill discovery
- one core skill invocation
- one backend read/write validation

- [ ] **Step 3: Run smoke tests**

Run: `pnpm --dir apps/cli run test:vitest -- golden-path`

Expected: pass in prepared CI/local environment.

- [ ] **Step 4: Commit**

```bash
git add apps/cli/src/tests/golden-path.pi-github.vitest.test.ts docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md
git commit -m "test: add pi+github golden-path smoke coverage and runbook"
```

### Task 7: Integrate golden-path checks into CI and distribution workflows

**Files:**
- Modify: `scripts/ci/build-kata-distributions.sh`
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/cli-release.yml`
- Modify: `.github/workflows/desktop-release.yml`

- [ ] **Step 1: Add golden-path smoke lane**

CI must run smoke tests in addition to existing lint/typecheck/test/build.

- [ ] **Step 2: Fail distribution builds on smoke regression**

`build-kata-distributions.sh` should return non-zero when golden-path checks fail.

- [ ] **Step 3: Verify pipeline locally**

Run:
- `bash scripts/ci/build-kata-distributions.sh`
- `pnpm run validate:affected`

Expected: artifact and behavior checks both pass.

- [ ] **Step 4: Commit**

```bash
git add scripts/ci/build-kata-distributions.sh .github/workflows/ci.yml .github/workflows/cli-release.yml .github/workflows/desktop-release.yml
git commit -m "ci: gate kata releases on golden-path behavior checks"
```

### Task 8: Desktop integrated proof and sign-off evidence

**Files:**
- Modify: `apps/desktop/src/main/kata-backend-client.ts` (only if validation reveals mismatch)
- Modify: `apps/desktop/src/main/pi-agent-bridge.ts` (only if validation reveals mismatch)
- Modify: `docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md`

- [ ] **Step 1: Execute desktop runbook against same GitHub project**

Validate:
- bundled runtime resolution
- direct Pi RPC path
- skill/runtime/backend operations match CLI golden path

- [ ] **Step 2: Record evidence in runbook**

Add dated evidence section with:
- command outputs
- observed behavior
- known caveats

- [ ] **Step 3: Regression check**

Run:
- `pnpm --dir apps/desktop run test`
- `pnpm run validate:affected`

Expected: desktop tests remain green after any fixes.

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src/main/kata-backend-client.ts apps/desktop/src/main/pi-agent-bridge.ts docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md
git commit -m "docs(desktop): capture integrated golden-path validation evidence"
```

---

## Completion Checklist

- [ ] Capability matrix has no `unknown` rows.
- [ ] Core skills are emitted and enforced by tests.
- [ ] `setup --pi` performs real install + verification.
- [ ] Runtime uses live GitHub Projects v2 adapter for golden-path operations.
- [ ] Label mode is fully removed from code, types, and docs.
- [ ] Manual runbook passes on clean environment.
- [ ] CI blocks merge/release on golden-path failures.

## Recommended Execution Order

1. Task 1
2. Task 2
3. Task 3
4. Task 4
5. Task 5
6. Task 6
7. Task 7
8. Task 8

## Notes for Parallelization

- Tasks 2 and 5 can run in parallel (different surfaces).
- Task 6 can start after Task 4 and Task 5 land.
- Task 7 should land only after Task 6 is green.
- Task 8 is final verification/sign-off.
