---
type: Verify Report
title: A4 PR2 Formal Review Publication Verify Report
status: In verification
description: Automated and preview-only Ratatui verification for A4 PR2 formal review publication and routing; live automatic publication and full restart matrix remain open.
tags: [symphony, review-stage, a4, pr2, verify]
timestamp: 2026-08-02T16:40:00Z
---

# A4 PR2 Formal Review Publication — Verify Report

## Status

**In verification** — the feature branch implements deterministic formal review publication, durable routing, changed-head cycles, operator recovery, and doctor permission probing. Automated validation and a fresh preview-only Ratatui run passed. Automatic formal-review UAT remains separate from the mainline/UAT preview workflow.

Branch: `feat/a4-review-publication`

## Automated evidence

- `cargo fmt --check` — Pass
- `cargo clippy -- -D warnings` — Pass
- `cargo test -- --test-threads=1` — Pass: 403 library tests plus all integration suites. An unconstrained parallel run showed intermittent pre-existing Codex/mock-server timing failures in `orchestrator_tests`; the serial gate passed all tests.
- `pnpm run validate:affected` — Pass: 2 Turborepo tasks, including the Symphony suite
- Formal review client tests cover multiline anchors, marker adoption, foreign identity conflicts, stale heads, invalid permission probes, and unexpected successful permission probes.
- Store tests cover draft-PR eligibility, changed-head retry budgets, conflict recovery, publication leases, migration reapplication, and publication-step persistence.
- Coordinator tests cover retry-ceiling classification and live revision guards.

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

## Acceptance coverage

Implemented and automated:

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

## Open verification work

1. Run automatic formal-review UAT on an isolated feature workflow with a disposable or explicitly approved UAT issue/PR.
2. Capture restart evidence at each publication boundary: after review creation, findings recording, route update, and before finalization.
3. Exercise a real changed-head re-review and verify resolved, persisting, and new findings on the remote PR.
4. Confirm foreign-review conflict listing and reset over the live HTTP operator path.
5. Update the A4 design and roadmap to Completed only after these checks pass.

Mainline and the maintained UAT workflow remain preview-only until the automatic path is accepted.
