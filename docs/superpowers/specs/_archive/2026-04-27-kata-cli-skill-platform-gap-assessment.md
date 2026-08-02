---
type: Reference
title: Kata Cli Skill Platform Gap Assessment
description: "Archived reference document: Kata Cli Skill Platform Gap Assessment."
tags: [archive, cli]
timestamp: 2026-07-15T20:00:00Z
---

# Kata CLI Skill Platform Gap Assessment

Date: `2026-04-27`

## Executive Status

The branch is now stabilized for the agreed golden path:

1. Pi harness integration via `@kata-sh/cli setup --pi`
2. Harness-agnostic core Kata skills (8 surfaced core workflows)
3. Typed standalone CLI runtime contract over JSON operations
4. GitHub Projects v2 backend path (label mode removed)
5. CI/release gating that includes golden-path behavior checks

Current workflow-level status is tracked in:
- [2026-04-27-kata-cli-capability-matrix.md](./2026-04-27-kata-cli-capability-matrix.md)

Manual and automated validation instructions/evidence are tracked in:
- [2026-04-27-kata-cli-manual-validation-runbook.md](./2026-04-27-kata-cli-manual-validation-runbook.md)

## Current Readiness

| Area | Status | Notes |
| --- | --- | --- |
| Pi setup/doctor flow | `Implemented` | `setup --pi` performs real install/config writes; `doctor` performs actionable checks. |
| Standalone CLI runtime contract | `Implemented` | Runtime exposes typed operations for project/milestone/slice/task/artifact/execution. |
| GitHub Projects v2 policy | `Implemented` | Projects v2-only enforcement now applied across CLI/desktop-facing config paths. |
| Core skills surface | `Implemented (core)` | 8 core workflows are emitted with workflow + contract metadata. |
| Desktop direct Pi RPC path | `Implemented` | Desktop uses Pi directly in RPC mode and uses shared typed domain API. |
| CI/release behavior gating | `Implemented` | Golden-path smoke checks required in CI/release scripts. |
| Full workflow corpus migration | `Pending` | 21 workflows remain non-surfaced (`pending`) by matrix classification. |
| Workflow-level behavioral validation | `Pending` | Core is contract-ready, but per-workflow behavioral proof is still limited. |

## What Is Now Done

### 1. Runtime and Backend Contract

- `apps/cli/src/transports/json.ts` now supports:
  - `project.getContext`
  - `milestone.getActive`
  - `slice.list`
  - `task.list`
  - `artifact.list`
  - `artifact.read`
  - `artifact.write`
  - `execution.getStatus`
- `apps/cli/src/cli.ts` routes supported operations through `resolveBackend(...)` + `createKataDomainApi(...)` with structured error handling.
- Label mode removed from GitHub runtime policy; Projects v2 is required.

### 2. Pi Setup and Health

- `apps/cli/src/commands/setup.ts` now performs real Pi installation behavior:
  - skill copy/install
  - Pi settings update (`skills`, `enableSkillCommands`)
  - managed manifest generation and stale-entry pruning
- `apps/cli/src/commands/doctor.ts` now validates:
  - CLI binary
  - skill source
  - Pi skills dir
  - Pi install marker
  - Pi settings
  - backend config parseability

### 3. Skills and Coverage

- `apps/orchestrator/skills-src/manifest.json` contains normalized core metadata:
  - `workflow`
  - `contractOperations`
  - `setupHint`
  - `runtimeRequired`
- `apps/orchestrator/scripts/build-skill-bundle.js` validates manifest/workflow/runtime metadata invariants.
- Core emitted skills are enforced by tests.

### 4. CI/Release Gating

- `scripts/ci/build-kata-distributions.sh` now enforces golden-path smoke behavior checks.
- `.github/workflows/ci.yml` includes a required `golden-path-smoke` job.
- CLI and desktop release workflows include golden-path validation gates.

## What Is Still Left

### 1. Workflow Expansion Beyond Core 8

- 21 workflows remain `pending` in the capability matrix (not surfaced as harness-facing skills yet).
- Next tranche should be selected intentionally (not bulk-exported blindly), with explicit contract-operation mapping.

### 2. Workflow-Level Behavioral Validation

- Core workflows are contract-ready, but validation evidence is still mostly contract-level.
- Add workflow-specific execution tests/evidence for each surfaced core skill before claiming full migration completeness.

### 3. Harness Adapter Completion Beyond Pi

- Pi setup path is implemented.
- Codex/Claude/Cursor/skills.sh packaging/install paths still need full installer + verification flows (template scaffolding exists).

### 4. User-Facing Docs Consolidation

- Align all user-facing docs with the current runtime reality:
  - setup/doctor flow
  - Projects v2-only policy
  - core skill surface and runbook steps

## Validation Evidence (Latest Local)

- `pnpm --dir apps/orchestrator run build:skills`: pass
- `pnpm --dir apps/orchestrator run test`: pass
- `pnpm --dir apps/cli run test:vitest -- golden-path`: pass
- `pnpm --dir apps/desktop run typecheck`: pass
- `pnpm run validate:affected`: pass

## Recommended Next Sequence

1. Expand surfaced skills from `pending` workflows in small batches (3-5 each).
2. For each batch, add workflow-specific behavioral validation evidence.
3. Complete non-Pi harness installer adapters with the same typed CLI contract boundary.
4. Keep the capability matrix current as the release-control artifact.
