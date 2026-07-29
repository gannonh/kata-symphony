---
type: Build Report
title: A3 PR2 Deterministic Draft-PR Publication Build Report
status: Implemented
description: Build completion report for A3 PR2 — expected-projection branch push, owned draft PR, Agent Review handoff, HTTP pull_request, progressive publication durability.
tags: [symphony, implementation-stage, a3, pr2, build]
timestamp: 2026-07-29T18:00:00Z
---

# A3 PR2 Deterministic Draft-PR Publication — Build Report

## Status

Implemented — review remediation and automated gates Pass; live GitHub draft-PR / restart UAT residual (see [verify report](2026-07-29-a3-pr2-verify-report.md)). Pull request: [#607](https://github.com/gannonh/kata-symphony/pull/607).

## Spec

- Spec: [`2026-07-26-a3-implementation-stage-design.md`](2026-07-26-a3-implementation-stage-design.md)
- Scope: PR2 delivery slice (AC 14–16 and related HTTP/events/tests; automated portions of 17–19)
- Non-goals: live UAT, Docker daemon lifecycle, A4 agent review
- Branch: `cursor/a3-pr2-draft-pr-publication-4eda`
- Base: `main` @ `878bb4b4` (A3 PR1)

## Tasks completed

1. GitHub client `list_pull_requests` / `create_pull_request` + mockito tests
2. Migration `005_implementation_draft_pr.sql` + progressive publication store APIs + draft-PR artifacts
3. Branch expected-projection publisher (never force) with bare-remote tests
4. Draft PR create-before-record recovery, foreign/closed/ready/drift rejection
5. Tracker handoff only after verified draft-PR artifact; remove approval label; set Agent Review
6. Coordinator automatic mode wiring + `TriageRoutingPort` / pull-request ports
7. HTTP `pull_request` object; doctor automatic completion_route checks
8. OKF roadmap/docs updates

## Review remediation

The PR takeover closed the publication-path findings before re-review:

1. Publication intents pin forge owner, repository, base, and resolved branch; reconciliation rejects configuration drift.
2. The trusted publisher derives a clean HTTPS forge URL instead of reusing the local workspace path.
3. Git fetch, push, and observation receive the configured GitHub token through a subprocess-only authorization header. Credentials are never stored in the remote URL or Git config.
4. Network Git commands have a finite timeout, disabled terminal prompting, bounded output capture, and credential/remote redaction.
5. Persisted draft-PR artifacts are checked against live GitHub state again before tracker routing.
6. Closed owned PRs no longer poison recovery; retryable issue drift stays pending.
7. Authenticated-login failures propagate, missing publication intents fail loudly, and doctor reports the pinned publication target without claiming unverified permissions.
8. Focused regression tests cover every repaired failure and the preview path remains backward-compatible.

## Files changed (primary)

- `apps/symphony/src/github/client.rs`, `tests/github_client_tests.rs`
- `apps/symphony/src/implementation/{automatic,branch,comment,coordinator,publisher,domain,mod}.rs`
- `apps/symphony/src/triage/migrations/005_implementation_draft_pr.sql`, `store.rs`, `runtime.rs`
- `apps/symphony/src/http_server.rs`, `doctor.rs`, `workspace.rs`
- OKF: design status, specs index, PRD A3 table, ADR-0004 note, this build + verify report

## Tests and verification

```bash
cd apps/symphony && cargo fmt
cd apps/symphony && cargo clippy --lib -- -D warnings
cd apps/symphony && cargo test --lib
```

Results (GitHub Actions on the remediated PR head):

| Gate | Result |
| --- | --- |
| `cargo fmt --check` | Pass |
| `cargo clippy -- -D warnings` | Pass |
| `cargo test` | Pass, including **327** library tests and **14** GitHub client tests |
| `cargo llvm-cov --fail-under-lines 72` | Pass |
| GitHub backend validation / golden-path smoke / distributions | Pass |
| Docker daemon | Unavailable — residual |
| Live GitHub draft-PR UAT | Not run — residual |

## Approved deviations

- Live GitHub draft-PR create / Agent Review Projects v2 mutation / restart-during-publication UAT not run in this environment
- Docker daemon unavailable for full container UAT (unchanged from PR1)

## Known follow-ups

- Live UAT on `uat-symphony` Project #16 (AC 20 PR2 portion)
- A4 agent code review stage
