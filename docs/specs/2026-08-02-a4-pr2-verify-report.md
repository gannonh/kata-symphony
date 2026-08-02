---
type: Verify Report
title: A4 PR2 Formal Review Publication Verify Report
status: Verified with residuals
description: Automated, live automatic, restart-matrix, and Ratatui verification for A4 PR2 formal review publication and routing; live worker credential-isolation and broader Docker evidence remain residual.
tags: [symphony, review-stage, a4, pr2, verify]
timestamp: 2026-08-02T16:40:00Z
---

# A4 PR2 Formal Review Publication — Verify Report

## Status

**Verified with residuals** — the feature branch implements deterministic formal review publication, durable routing, changed-head cycles, operator recovery, active lease fencing, and doctor permission probing. Automated validation, live automatic reconciliation, restart-boundary recovery, and a direct Ratatui run passed. Mainline and the maintained UAT workflow remain preview-only.

Branch: `feat/a4-review-publication`

## Automated evidence

- `cargo fmt --check` — Pass
- `cargo clippy -- -D warnings` — Pass
- `cargo test -- --test-threads=1` — Pass: 410 library tests plus all integration suites.
- `pnpm run validate:affected` — Pass: 2 Turborepo tasks, including the Symphony suite; no TypeScript files changed in the final fencing fix.
- Formal review client tests cover multiline anchors, marker adoption, foreign identity conflicts, stale heads, invalid permission probes, and unexpected successful permission probes.
- Store tests cover draft-PR eligibility, changed-head retry budgets, conflict recovery, active and expired publication leases, terminal stale writers, supersession retry preservation, migration reapplication, and publication-step persistence.
- Publisher tests cover marker adoption, foreign identity conflicts, stale heads, preview lease ownership, and restart-safe progressive publication.
- Coordinator paths claim before failure recording, changed-head supersession, Projects v2 routing, and finalization.

## Doctor evidence

A fresh `symphony doctor` run against `gannonh/uat-symphony` Project #16 passed the formal review permission probe against pull request #46:

- `GitHub Review Auth` — Pass
- `GitHub Review Project` — Pass
- `GitHub Review Routes` — Pass
- `GitHub Review API` — Pass: review endpoint authorization verified
- Exit status: `0`

The probe posts an intentionally invalid review event and accepts GitHub's validation response (`422`) as endpoint authorization without creating a review.

## Ratatui UAT evidence

A fresh direct Ratatui run used the feature binary and the existing preview-only UAT workflow. Symphony started with the HTTP observability server and active supervisor, completed startup workspace scanning, and reported zero active candidates without mutating the configured preview workflow.

Evidence bundle: `/tmp/kata-symphony-current-uat-evidence/a4-pr2/`

- `state.json` contains the Project #16 URL, empty active queues, and `supervisor.status=active` after the current feature binary reached its first poll.
- `doctor.txt` contains the complete preflight output.
- `tui-session.ansi` and `tui-session-initial.ansi` contain the direct Ratatui terminal captures.
- `logs/log/symphony.log` contains startup, HTTP binding, TUI enablement, and review poll events.
- The SQLite lock was available after shutdown.

A latest-branch automatic reconciliation used commit `5cd42db1` and a seeded copy of the formal SQLite database. It adopted the existing marker-owned review `4839451136`, applied all four publication steps, moved the Project item to `Human Review`, then restored it to `Agent Review`. Evidence bundle: `/tmp/kata-symphony-current-uat-evidence/a4-pr2/formal-latest/`.

- `uat-summary.json` records the applied durable projection, single remote review, route restoration, head/base SHAs, and known warnings.
- `db-summary.json`, `pull-request.json`, and `project-item-after-restore.json` provide durable/API read-backs.
- `doctor-latest.txt` exits `0`; its explicit warnings are documented in `uat-summary.json`.
- `symphony.log` and `tui-session.ansi` capture the direct Ratatui run with HTTP observability and the active supervisor.

A fresh manual formal UAT used the current branch binary and UAT workflow with issue [#47](https://github.com/gannonh/uat-symphony/issues/47) and draft PR [#48](https://github.com/gannonh/uat-symphony/pull/48). The real review worker produced one blocking scope finding. Symphony published one authenticated `COMMENTED` review (`4839847481`), applied `review_created`, `findings_recorded`, `route_applied`, and `comment_final` with `retry_count=0`, and routed Project #16 to `Rework`. After a stop/start restart, reconciliation kept one remote review and the applied intent; no duplicate publication occurred. The fixture, branch, issue, and Project item were cleaned up after capture. Evidence bundle: `/tmp/kata-symphony-current-uat-evidence/a4-pr2/manual-47/`.

## Acceptance coverage

Implemented, automated, and live-verified where marked:

1. Durable A3 draft and draft-only eligibility
2. Review attempt ownership and per-head retry accounting
3. Worker credential boundary and manifest validation from PR1
4. Atomic formal review payloads with marker ownership
5. Create-before-record adoption and durable publication steps
6. Cross-process publication claim lease
7. Foreign-marker conflict recovery through the existing operator API
8. Route selection, Projects v2 read-back, and HTTP projections
9. Changed-head waiting semantics and stale-creation protection
10. Finding carry-forward and persisting-finding suppression
11. Doctor validation of formal review endpoint authorization
12. Live automatic formal review reconciliation on `gannonh/uat-symphony` PR #46, with one existing marker-owned review adopted and no duplicate review created
13. Live restart matrix for `create-before-record`, `after-review-created`, and `after-findings-recorded`, each recovering to `applied` with `retry_count=0`
14. Live Projects v2 route mutation to `Human Review`, followed by restoration to `Agent Review`
15. Active-lease CAS fencing for identity, route, step, error, preview completion, finalization, and changed-head supersession
16. Live formal blocking-finding publication, `Rework` routing, and restart reconciliation with one remote review

## Residual verification work

1. Capture a dedicated live worker credential-isolation proof bundle across the formal worker process boundary.
2. Capture broader Docker execution evidence for the review worker; the existing Rust Docker integration suite passes.
3. Exercise a real changed-head re-review with changed remote content and verify resolved, persisting, and new findings on the remote PR.
4. Confirm foreign-review conflict listing and reset over the live HTTP operator path.

Mainline and the maintained UAT workflow remain preview-only. Formal evidence is isolated under `/tmp/kata-symphony-current-uat-evidence/a4-pr2/formal-latest/`.
