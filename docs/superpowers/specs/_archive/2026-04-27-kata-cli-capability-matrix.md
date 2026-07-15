---
type: Reference
title: Kata Cli Capability Matrix
description: Archived reference document: Kata Cli Capability Matrix.
tags: [archive, cli]
timestamp: 2026-07-15T20:00:00Z
---

# Kata CLI Capability Matrix

Date: `2026-04-27`  
Scope: every workflow file under `apps/orchestrator/kata/workflows/` (`34` total)

## Migration Rules

`contract-ready = skill surfaced + required runtime contract operations exposed`

`migrated = contract-ready + workflow-level validation evidence recorded`

Status labels:

- `core`: golden-path capability target in the current migration plan.
- `internal-only`: helper workflow not intended as a top-level skill surface.
- `consolidated`: behavior represented through another entrypoint.
- `pending`: not yet surfaced/validated as a migrated capability.

Evidence keys:

- `VB1`: `apps/orchestrator/tests/build-skill-bundle.test.js`
- `VB2`: `apps/orchestrator/tests/skill-coverage-matrix.test.js`
- `GP1`: `apps/cli/src/tests/golden-path.pi-github.vitest.test.ts`
- `GP2`: `pnpm run validate:affected` (latest local run: pass on `2026-04-27`)
- `MR1`: manual runbook at `docs/superpowers/specs/2026-04-27-kata-cli-manual-validation-runbook.md`

Runtime contract operation support is implemented in `apps/cli/src/transports/json.ts` via:
`project.getContext`, `milestone.getActive`, `slice.list`, `task.list`, `artifact.list`, `artifact.read`, `artifact.write`, `execution.getStatus`.

## Workflow Matrix

| Workflow | Legacy command | Emitted skill | Contract operations | Status | Contract-ready | Validation evidence |
| --- | --- | --- | --- | --- | --- | --- |
| add-phase | kata:add-phase | None | milestone.getActive; slice.list; artifact.read; artifact.write | pending | No | None |
| add-tests | kata:add-tests | None | milestone.getActive; task.list; artifact.read; artifact.write | pending | No | None |
| add-todo | kata:add-todo | None | milestone.getActive; artifact.read; artifact.write | pending | No | None |
| audit-milestone | kata:audit-milestone | None | milestone.getActive; slice.list; task.list; artifact.read | pending | No | None |
| check-todos | kata:check-todos | None | milestone.getActive; artifact.read | pending | No | None |
| cleanup | kata:cleanup | None | artifact.list; artifact.read; artifact.write | pending | No | None |
| complete-milestone | kata:complete-milestone | None | milestone.getActive; artifact.read; artifact.write | pending | No | None |
| diagnose-issues | None | None | execution.getStatus; task.list; artifact.read; artifact.write | consolidated | No | None |
| discovery-phase | None | None | project.getContext; artifact.read; artifact.write | pending | No | None |
| discuss-phase | kata:discuss-phase | kata-discuss-phase | project.getContext; milestone.getActive; artifact.read; artifact.write | core | Yes | VB1, VB2, GP1, GP2, MR1 (contract-level) |
| execute-phase | kata:execute-phase | kata-execute-phase | project.getContext; milestone.getActive; slice.list; task.list; artifact.read; execution.getStatus | core | Yes | VB1, VB2, GP1, GP2, MR1 (contract-level) |
| execute-plan | None | None | task.list; artifact.read; artifact.write; execution.getStatus | internal-only | No | None |
| health | kata:health | kata-health | project.getContext; milestone.getActive; execution.getStatus | core | Yes | VB1, VB2, GP1, GP2, MR1 (contract-level) |
| help | kata:help | None | project.getContext | pending | No | None |
| insert-phase | kata:insert-phase | None | milestone.getActive; slice.list; artifact.read; artifact.write | pending | No | None |
| list-phase-assumptions | kata:list-phase-assumptions | None | milestone.getActive; artifact.read | pending | No | None |
| map-codebase | kata:map-codebase | None | project.getContext; artifact.write | pending | No | None |
| new-milestone | kata:new-milestone | None | project.getContext; milestone.getActive; artifact.read; artifact.write | pending | No | None |
| new-project | kata:new-project | kata-new-project | project.getContext; artifact.write | core | Yes | VB1, VB2, GP1, GP2, MR1 (contract-level) |
| pause-work | kata:pause-work | None | execution.getStatus; artifact.write | pending | No | None |
| plan-milestone-gaps | kata:plan-milestone-gaps | None | milestone.getActive; slice.list; task.list; artifact.read; artifact.write | pending | No | None |
| plan-phase | kata:plan-phase | kata-plan-phase | project.getContext; milestone.getActive; slice.list; task.list; artifact.read; artifact.write | core | Yes | VB1, VB2, GP1, GP2, MR1 (contract-level) |
| progress | kata:progress | kata-progress | project.getContext; milestone.getActive; slice.list; task.list; execution.getStatus | core | Yes | VB1, VB2, GP1, GP2, MR1 (contract-level) |
| quick | kata:quick | kata-quick | project.getContext; artifact.write | core | Yes | VB1, VB2, GP1, GP2, MR1 (contract-level) |
| remove-phase | kata:remove-phase | None | milestone.getActive; slice.list; artifact.read; artifact.write | pending | No | None |
| research-phase | kata:research-phase | None | milestone.getActive; artifact.read; artifact.write | pending | No | None |
| resume-project | kata:resume-work | None | project.getContext; milestone.getActive; slice.list; task.list; artifact.read | consolidated | No | None |
| set-profile | kata:set-profile | None | project.getContext; execution.getStatus | pending | No | None |
| settings | kata:settings | None | project.getContext; execution.getStatus | pending | No | None |
| transition | None | None | milestone.getActive; execution.getStatus | internal-only | No | None |
| update | kata:update | None | project.getContext; execution.getStatus | pending | No | None |
| validate-phase | kata:validate-phase | None | milestone.getActive; task.list; artifact.read; artifact.write | pending | No | None |
| verify-phase | None | None | milestone.getActive; slice.list; task.list; artifact.read; artifact.write | internal-only | No | None |
| verify-work | kata:verify-work | kata-verify-work | project.getContext; task.list; artifact.list; artifact.read; execution.getStatus | core | Yes | VB1, VB2, GP1, GP2, MR1 (contract-level) |

## Snapshot Totals

- Total workflows: `34`
- Status counts: `core=8`, `internal-only=3`, `consolidated=2`, `pending=21`
- Workflow-linked surfaced skills: `8`
- Contract-ready workflows: `8` (all core workflows)
- Fully migrated workflows by strict rule (`workflow-level` validation evidence): `0`

## Interpretation

The platform is now contract-ready for the core golden path (`Pi + Skills + standalone CLI + GitHub Projects v2`), including runtime operation coverage and CI gating.  
Remaining work is primarily workflow-level behavioral validation and expansion beyond the core eight surfaced workflows.
