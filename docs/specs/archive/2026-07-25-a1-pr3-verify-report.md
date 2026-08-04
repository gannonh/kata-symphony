---
type: Verify Report
title: A1 PR3 Recovery and Agreement Measurement Verify Report
description: Live GitHub Verify results for A1 PR3 restart recovery, retry evidence, route correction, and agreement metrics.
tags: [symphony, triage, a1, pr3, verify, uat, github]
timestamp: 2026-07-25T17:14:00Z
status: Completed
source_status: completed
migrated: false
archived_at: 2026-08-04T19:34:07Z
---

> **Completed before migration** (source status: completed). Retained as history. Not tracked in GitHub Issues.

# A1 PR3 Recovery and Agreement Measurement — Verify Report

## Status

**Rejected** — correction measurement, metrics, durable retry records, preview-to-automatic publication, and restart retry passed, but restart recovery did not terminate the live orphaned child. PR3 does not satisfy its interrupted-process recovery merge gate.

## Scope and environment

- Spec: [A1 GitHub Issue Triage](2026-07-16-a1-github-issue-triage-design.md), PR3 portions of criteria 4 and 14–18
- Build: [A1 PR3 build report](2026-07-25-a1-pr3-build-report.md)
- Code under test: `2f0d210`
- Runtime/backend: Symphony runtime against GitHub Projects v2
- Target: `gannonh/uat-symphony`, Project [#16](https://github.com/users/gannonh/projects/16)
- Local evidence: `uat-evidence/a1-pr3-symphony-github-20260725170510/`
- Standard backend evidence: `uat-evidence/symphony-runtime-github-20260725170315-79465/`

The standard backend proof passed `symphony doctor`, all seven shared helper operations, provider reads, and proof-link capture. Three PR-only helper operations were explicitly skipped because the checkout had no discoverable pull request.

## Acceptance results

| Area | Result | Evidence |
| --- | --- | --- |
| AC4 durable attempt/retry | Pass | #17 attempt 1 became `interrupted`; attempt 2 completed against the same issue/configuration revision |
| AC4 interrupted child termination | **Fail** | PID/PGID `81528` remained alive after restart recovery |
| AC14 bounded failure records | Pass | #16 exposed three bounded `triage_setup_failed` records before a new configuration revision succeeded |
| AC15 run/metrics API | Pass | Exact run records remained readable; metrics reported two routes and one correction |
| AC16 live/durable correction | Pass | WebSocket sequence `197` and durable event `019f9a42-f1c0-7b50-9f9c-4064d9613c72` recorded #17 `implement` → `spec` |
| AC16 correction dedupe | Pass | Three later polls retained one observation/event, `correction_count: 1`, `correction_rate: 0.5` |
| AC17 automated coverage | **Fail (flaky)** | Final affected gate failed 1/229 at the recovery test's signalability precondition; 21 focused reruns then passed |
| AC18 live fixtures | Partial | Expected routes, clarification, preview/automatic publication, off-project diagnostic, restart/retry, and correction observed; orphan termination failed |

## Live fixtures

- [#16](https://github.com/gannonh/uat-symphony/issues/16) — underspecified performance request routed `needs_information`; [preview](https://github.com/gannonh/uat-symphony/issues/16#issuecomment-5079413577), [automatic publication](https://github.com/gannonh/uat-symphony/issues/16#issuecomment-5079423503)
- [#17](https://github.com/gannonh/uat-symphony/issues/17) — exact documentation replacement routed `implement`; [off-project diagnostic](https://github.com/gannonh/uat-symphony/issues/17#issuecomment-5079415072), [preview](https://github.com/gannonh/uat-symphony/issues/17#issuecomment-5079420439), [automatic publication](https://github.com/gannonh/uat-symphony/issues/17#issuecomment-5079423190)

Both automatic publications completed `comment_pending`, `route_label`, `project_state`, `conflict_cleanup`, `comment_route_effects`, `intake_removed`, and `comment_applied`. The #17 route was then changed from `ready-for-agent` to `ready-to-spec`.

## Blocking recovery defect

Before the hard stop, the durable attempt stored:

- PID/PGID: `81528`
- Linux start token: `577506`
- executable identity: `/usr/bin/bash`
- disposable workspace/output paths

After Symphony PID `81289` was killed, the child remained alive under PID 1. Its shell had executed `/usr/bin/sleep`; the live executable resolved as `/usr/bin/coreutils`. On restart, Symphony correctly:

1. marked attempt `019f9a40-044f-7c50-95d9-35e84f6f3517` interrupted;
2. removed its disposable attempt directory;
3. completed retry `019f9a41-6c5f-78a2-a60d-a410dc1be445`.

However, exact executable matching rejected the legitimate post-`exec` child identity, so recovery skipped process-group signaling. PID `81528` remained live and required explicit manual termination. This can also affect launchers whose executable changes after spawn.

The final `pnpm run validate:affected` also failed `triage::coordinator::tests::recovery_terminates_orphaned_child_and_removes_attempt_directory` at `precondition: the orphan must be identifiable and signalable` (228 passed, 1 failed). One immediate focused rerun and 20 additional focused stress runs passed, confirming nondeterministic suite-level recovery-test behavior rather than a consistently failing assertion.

## UAT environment findings

The UAT repository contains tracked `.symphony/workspaces` gitlinks without matching `.gitmodules` entries. Repository-integrity setup therefore failed three times. No remote repository content was changed; Verify continued with a local-only sanitized clone and a new configuration revision.

The bundled cleanup evidence omitted the overridden repository coordinates. Its first safety check correctly skipped all records instead of closing uncertain targets. Cleanup succeeded after rerunning with explicit `gannonh/uat-symphony` coordinates.

All UAT-created issues (#13–#17) are closed. The exact orphan process group was manually terminated; the process remained defunct pending PID 1 reaping.

## Recommendation

Do not merge PR3 as verified. Fix recovery identity handling for legitimate executable changes across an `exec` chain, stabilize the recovery test under the full suite, add a regression test using a launcher script that `exec`s another binary, and rerun the restart portion of live UAT. The correction/metrics evidence can be retained.
