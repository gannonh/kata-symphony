# Specs

Specs for this project are GitHub Issues. This directory holds no spec documents.

## Read the roadmap

```bash
gh issue list --label kind:spec --state open            # all active specs
gh issue list --label status:approved --state open      # approved, ready to build
gh issue list --label status:implemented --state open   # built, awaiting verification
gh issue view <N>                                       # read a spec
gh sub-issue list <N>                                   # read an epic's phases
```

## Status model

| Label                | Meaning                                     |
| -------------------- | ------------------------------------------- |
| `status:draft`       | Being written or revised. Do not build.     |
| `status:approved`    | Approved by the maintainer. Ready to build. |
| `status:implemented` | Built and reported. Ready to verify.        |
| `status:verified`    | Acceptance evidence accepted.               |
| `status:blocked`     | Cannot proceed. See the issue body.         |

## Writing and executing specs

Use the `plan-build-verify-github` skill. It publishes specs as issues, runs Build
against approved issues, and posts acceptance evidence back to the issue.

## Archive

Pre-migration spec files are preserved under [`archive/`](./archive/) with links to
their issues. Completed specs were archived without an issue. Both are history and
are not maintained.

## Roadmap context carried over from the previous index

The content below predates the migration to GitHub Issues. Links to issues
remain valid; links to spec files now point into `archive/`. Reconcile this
section against the GitHub roadmap, then delete it.

Roadmap for planned and completed work. Historical Superpowers designs and plans remain under [`../superpowers/`](../superpowers/); this index is the OKF entry point.

# Active

* [Symphony Software Factory Platform PRD](archive/symphony-software-factory-platform-prd.md) - product requirements and vertical-slice roadmap for the full software factory platform
* [A3 Implementation Stage](archive/2026-07-26-a3-implementation-stage-design.md) - Complete; **PR1+PR2 shipped** (PR1 [#606](https://github.com/gannonh/kata-symphony/pull/606); PR2 [#607](https://github.com/gannonh/kata-symphony/pull/607) (`d456c051`); [build](archive/2026-07-29-a3-pr2-build-report.md), [verify](archive/2026-07-29-a3-pr2-verify-report.md) Verified with residuals; [ADR-0004](../adrs/0004-a3-implementation-durability-and-bundles.md)); direct draft-PR, Agent Review preview, and Ratatui TUI UAT verified on Project #16
* [A4 Agent Review Stage](archive/2026-07-30-a4-agent-review-stage-design.md) - **Active**; PR1 is implemented and PR2 is implemented on `feat/a4-review-publication`; live automatic publication, restart-matrix recovery, and active-lease fencing are verified with worker credential-isolation and broader Docker evidence residuals ([PR2 verify report](archive/2026-08-02-a4-pr2-verify-report.md); [ADR-0005](../adrs/0005-a4-review-publication-fencing.md))
* [Pi Symphony Extension Design](../superpowers/specs/2026-05-14-pi-symphony-extension-design.md) - Pi extension to init, launch, monitor, and steer Symphony
* [Wave 4 Shared Context and Diagnostics Plan](../superpowers/plans/2026-05-17-wave-4-symphony-shared-context-diagnostics.md) - dashboard parity for shared context + diagnostics

# Planned

* A4 PR2 formal review publication and routing — implemented and verified with residuals on `feat/a4-review-publication`; worker credential-isolation proof and broader Docker evidence remain ([design](archive/2026-07-30-a4-agent-review-stage-design.md); [verify](archive/2026-08-02-a4-pr2-verify-report.md); [ADR-0005](../adrs/0005-a4-review-publication-fencing.md); [PRD A4](archive/symphony-software-factory-platform-prd.md))

# Blocked

_(none)_

# Completed (recent)

* A3 PR2 deterministic draft-PR publication — shipped in [#607](https://github.com/gannonh/kata-symphony/pull/607) (`d456c051`) ([build](archive/2026-07-29-a3-pr2-build-report.md); [verify](archive/2026-07-29-a3-pr2-verify-report.md) Verified with residuals; direct GitHub/TUI UAT passed on Project #16)
* A3 PR1 implementation and validation preview — shipped in [#606](https://github.com/gannonh/kata-symphony/pull/606) ([build](archive/2026-07-29-a3-pr1-build-report.md), [verify](archive/2026-07-29-a3-pr1-verify-report.md))
* A1 GitHub Issue Triage — GitHub path complete: PR1 [#587](https://github.com/gannonh/kata-symphony/pull/587), PR2 [#598](https://github.com/gannonh/kata-symphony/pull/598), PR3 [#599](https://github.com/gannonh/kata-symphony/pull/599) (`c52d23dc`); Linear triage deferred ([design](archive/2026-07-16-a1-github-issue-triage-design.md); [ADR-0001](../adrs/0001-a1-triage-durability-and-isolation.md), [ADR-0002](../adrs/0002-triage-process-recovery-identity.md))
* A2 Spec Stage — live UAT accepted on `uat-symphony` Project #16 ([verify](archive/2026-07-26-a2-uat-verify-report.md); [ADR-0003](../adrs/0003-a2-spec-stage-artifacts-and-gates.md))
* A1 PR3 recovery and agreement measurement — shipped in [#599](https://github.com/gannonh/kata-symphony/pull/599) (`c52d23dc`) ([build](archive/2026-07-25-a1-pr3-build-report.md), [re-verify](archive/2026-07-25-a1-pr3-reverify-report.md))
* A1 PR2 automatic route publication — shipped in [#598](https://github.com/gannonh/kata-symphony/pull/598) (`6a454fe9`); Verify accepted on `uat-symphony` Project #16 ([verify](archive/2026-07-24-a1-pr2-verify-report.md))
* A1 PR1 triage preview — durable factory runs, intake, runner, preview comments, HTTP read API ([#587](https://github.com/gannonh/kata-symphony/pull/587); [ADR-0001](../adrs/0001-a1-triage-durability-and-isolation.md))

# Completed (archived)

Designs and plans under `_archive` are treated as completed historical work unless a newer active doc supersedes them.

## Specs archive

* [Kata CLI skill platform design](../superpowers/specs/_archive/2026-04-26-kata-cli-skill-platform-design.md)
* [Kata CLI capability matrix](../superpowers/specs/_archive/2026-04-27-kata-cli-capability-matrix.md)
* [Kata CLI manual validation runbook](../superpowers/specs/_archive/2026-04-27-kata-cli-manual-validation-runbook.md)
* [Kata CLI recovery stabilization design](../superpowers/specs/_archive/2026-04-27-kata-cli-recovery-stabilization-design.md)
* [Kata CLI skill platform gap assessment](../superpowers/specs/_archive/2026-04-27-kata-cli-skill-platform-gap-assessment.md)
* [Kata CLI skill platform realignment design](../superpowers/specs/_archive/2026-04-27-kata-cli-skill-platform-realignment-design.md)
* [Kata skills migration recovery design](../superpowers/specs/_archive/2026-04-28-kata-skills-migration-recovery-design.md)
* [CLI Linear core design](../superpowers/specs/_archive/2026-05-06-cli-linear-core-design.md)
* [GitHub Projects v2 state source of truth design](../superpowers/specs/_archive/2026-05-06-github-projects-v2-state-source-of-truth-design.md)
* [Symphony Linear execution design](../superpowers/specs/_archive/2026-05-06-symphony-linear-execution-design.md)
* [Symphony Linear execution and backend UAT design](../superpowers/specs/_archive/2026-05-07-symphony-linear-execution-and-backend-uat-design.md)
* [Symphony progress indicators design](../superpowers/specs/_archive/2026-05-15-symphony-progress-indicators-design.md)

## Plans archive

* [Kata CLI skill platform plan](../superpowers/plans/_archive/2026-04-26-kata-cli-skill-platform.md)
* [Kata CLI recovery stabilization plan](../superpowers/plans/_archive/2026-04-27-kata-cli-recovery-stabilization.md)
* [Kata CLI phase A real backend plan](../superpowers/plans/_archive/2026-04-28-kata-cli-phase-a-real-backend.md)
* [Kata skills migration recovery plan](../superpowers/plans/_archive/2026-04-28-kata-skills-migration-recovery.md)
* [CLI Linear core implementation plan](../superpowers/plans/_archive/2026-05-06-cli-linear-core-implementation-plan.md)
* [GitHub Projects v2 state source of truth implementation plan](../superpowers/plans/_archive/2026-05-06-github-projects-v2-state-source-of-truth-implementation-plan.md)
* [Symphony Linear execution implementation plan](../superpowers/plans/_archive/2026-05-06-symphony-linear-execution-implementation-plan.md)
* [Symphony Linear execution and backend UAT implementation plan](../superpowers/plans/_archive/2026-05-07-symphony-linear-execution-and-backend-uat-implementation-plan.md)
* [Pi Symphony extension slice 1 plan](../superpowers/plans/_archive/2026-05-14-pi-symphony-extension-slice-1.md)
* [Slice 2 worker operations plan](../superpowers/plans/_archive/2026-05-14-slice-2-worker-operations.md)
* [Symphony progress indicators plan](../superpowers/plans/_archive/2026-05-15-symphony-progress-indicators.md)
* [Wave 3 Symphony console escalations plan](../superpowers/plans/_archive/2026-05-16-wave-3-symphony-console-escalations.md)
* [Pi skills migration (typo filename retained)](../superpowers/plans/_archive/pi-skills-migraqtion.md)
