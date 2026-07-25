---
type: Build Report
title: A1 PR3 Recovery and Agreement Measurement Build Report
status: Implemented
description: Build completion report for A1 PR3 interrupted-attempt recovery, route correction measurement, and agreement metrics.
tags: [symphony, triage, a1, pr3, recovery, correction]
timestamp: 2026-07-25T00:00:00Z
---

# A1 PR3 Recovery and Agreement Measurement — Build Report

## Status

**Implemented** — automated gates pass. Live GitHub UAT (AC18 restart + correction evidence) is outstanding and belongs to Verify.

## Spec

- Spec path: [`2026-07-16-a1-github-issue-triage-design.md`](2026-07-16-a1-github-issue-triage-design.md)
- Scope: A1 PR3 delivery slice — the restart, retry, correction, and agreement portions of criteria 4 and 14–18
- Prior slice: [PR2 build report](2026-07-24-a1-pr2-build-report.md), [PR2 verify report](2026-07-24-a1-pr2-verify-report.md)
- Non-goals: A2 spec stage, Linear parity, Docker/SSH execution

## Git range

- Base SHA: `6a454fe93ee3f1358f3ead25642cbb107d87bc5e`
- Head SHA: `b2199b5`
- Commits: `09f5422`, `251003d`, `b2199b5`

## Tasks completed

1. Reject late output from an interrupted attempt (`store_artifact` status guard)
2. Record attempt process identity and disposable paths (`triage::process_identity`, runner spawn sink, store `record_attempt_process`)
3. Reclaim interrupted attempts: identity-matched bounded termination plus attempt-directory cleanup
4. Correction reconciler with `triage_route_corrected` events and durable-only consistency diagnostics
5. Agreement metrics verified through `triage_metrics` / `GET /api/v1/factory-runs/metrics?stage=triage`
6. Quality gate and reference documentation

## Files changed

- `apps/symphony/src/triage/process_identity.rs` (new)
- `apps/symphony/src/triage/correction.rs` (new)
- `apps/symphony/src/triage/migrations/002_route_observations.sql` (new)
- `apps/symphony/src/triage/coordinator.rs`
- `apps/symphony/src/triage/store.rs`
- `apps/symphony/src/triage/runner.rs`
- `apps/symphony/src/triage/runtime.rs`
- `apps/symphony/src/triage/mod.rs`
- `apps/symphony/docs/WORKFLOW-REFERENCE.md`

## Design decisions

- **Route-consistency diagnostic is durable-only.** The spec fixes the live event vocabulary to exactly nine names, while the PR3 slice lists "consistency events". Zero-or-several route labels writes a `triage_route_consistency` factory event and is excluded from the live `EventHub` vocabulary and from `correction_count`. Confirmed with the maintainer before implementation.
- **Correction dedupe uses a table, not event scanning.** `route_observations` has primary key `(artifact_id, kind, value)`, so the reconciler is idempotent across polls by construction.
- **Comparison uses the recorded mapping.** Both publication and correction read the five labels from the intent's `desired_effects`, so a live `WORKFLOW.md` reload cannot reinterpret an older publication.
- **Measurement suspends while intake is reapplied.** A re-added intake label means a new attempt is coming, so the old route is no longer the decision under measurement.

## Bugs found and fixed while building

- `store_artifact` unconditionally set the stage to `completed`, so late output could silently mark an attempt a restart had already abandoned as successful, corrupting retry accounting.
- `process_identity::capture` read the child's process group back after spawn, racing the child's `setpgid`. Under load it could record *Symphony's own* group. Recovery would then refuse to signal (failing safe, but never terminating real orphans). Group-leader children now record the known group id directly.

## Tests and verification

```bash
cd apps/symphony
cargo fmt --check                    # pass
cargo clippy -- -D warnings          # pass (documented gate)
cargo test                           # pass

# repository affected-package validation, same command as CI
pnpm exec turbo run lint typecheck test --affected   # 2 successful, 2 total
```

Results: **229** lib tests pass (**84** under `triage::`, up from 61 at PR2), plus all integration test binaries. The triage suite was run three consecutive times to confirm the process-signalling tests are not flaky.

New behaviour coverage:

| Area | Test |
| --- | --- |
| Late output rejected | `store::interrupted_attempt_cannot_be_completed_by_late_output` |
| Identity persisted for recovery | `store::interrupted_attempt_exposes_recorded_process_for_recovery` |
| Candidate query + dedupe | `store::lists_applied_automatic_publications_as_correction_candidates` |
| Live process identified | `process_identity::captures_identity_for_a_live_process` |
| Reused PID rejected | `process_identity::rejects_identity_whose_start_token_changed` |
| setpgid race | `process_identity::child_group_is_recorded_without_racing_setpgid` |
| Self-group protection | `process_identity::refuses_to_signal_symphonys_own_process_group` |
| Bounded termination | `process_identity::terminates_a_live_process_group` |
| Cleanup path safety | `process_identity::cleanup_root_accepts_only_runner_created_attempt_dirs` |
| Spawn reporting | `runner::reports_spawned_child_identity_and_attempt_paths` |
| Orphan recovery end to end | `coordinator::recovery_terminates_orphaned_child_and_removes_attempt_directory` |
| Correction once per artifact + metrics | `coordinator::human_route_swap_records_one_correction_across_repeated_polls` |
| Agreement records nothing | `coordinator::unchanged_route_label_records_no_correction` |
| Ambiguity is durable-only | `coordinator::ambiguous_route_labels_record_durable_diagnostic_only` |
| Intake reapplied suspends measurement | `coordinator::reapplied_intake_label_suspends_correction_measurement` |
| Route comparison semantics | five tests in `triage::correction` |

## Review gates

- Bundled TDD workflow followed: one behaviour test at a time, RED verified before each implementation step
- Independent subagent review: **not used** — no subagent was requested for this session; reviews were performed inline
- Spec compliance (self-check): PR3 merge-gate criteria covered in code and tests, except the live-UAT portion of AC18
- Code quality (self-check): no new clippy findings; `cargo clippy --all-targets` reports 5 pre-existing findings in `src/github/auth.rs` and `tests/*` that also fail at the base SHA under clippy 1.97

## Known gaps and follow-ups

- **Live UAT outstanding (AC18).** Restart and correction evidence against `gannonh/uat-symphony` is Verify-phase work; the maintainer confirmed that target.
- **Codex children are not process-group leaders.** `codex_session_start` does not call `process_group(0)`, so its child shares Symphony's group. Identity is recorded faithfully and recovery correctly refuses to signal it, meaning an orphaned Codex app-server is cleaned up by directory removal but not terminated. The Pi path is unaffected. Worth closing separately.
- **Non-Linux start tokens use `ps`.** Linux reads `/proc/<pid>/stat`; other platforms shell out to `ps -o lstart=`. Only the Linux path was exercised in this environment.
- Broader orchestrator integration test proving dispatch exclusion across config reload (carried over from PR2).

## Environment note

The sandbox had no Rust toolchain despite the repo's environment notes; stable Rust and `openssl-devel` were installed before any code was written, so every result above comes from a real build.

## Build handoff to Verify

Verify should cover the restart, retry, correction, and agreement portions of criteria 4 and 14–18, with live GitHub evidence on `gannonh/uat-symphony`.

## 2026-07-25 remediation addendum

The initial Verify run found that executable equality rejected a legitimate
launcher-to-worker `exec` transition and made the recovery test flaky. Commit
`66f3da1` now authorizes signaling with PID, process group, and OS start token,
retains executable identity as diagnostics, rechecks identity before signaling,
and verifies that no running group member remains. A deterministic FIFO-backed
launcher regression covers the exact failure shape.

Commit `424bd4b` makes Symphony UAT evidence self-contained and validates every
GitHub cleanup target before provider work. The automated remediation gates pass;
live re-verification remains gated on credential containment as recorded in the
[re-verify report](2026-07-25-a1-pr3-reverify-report.md).
