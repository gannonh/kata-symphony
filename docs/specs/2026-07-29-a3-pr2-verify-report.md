---
type: Verify Report
title: A3 PR2 Deterministic Draft-PR Publication Verify Report
status: Verified with residuals
description: Verify report for A3 PR2 — automated gates and direct GitHub draft-PR, Agent Review preview, and Ratatui TUI UAT passed; restart-during-publication, cleanup, and Docker evidence remain residuals.
tags: [symphony, implementation-stage, a3, pr2, verify]
timestamp: 2026-08-02T01:00:00Z
---

# A3 PR2 Deterministic Draft-PR Publication — Verify Report

## Status

**Verified with residuals** — automated gates and direct GitHub draft-PR, Agent Review preview, and Ratatui TUI UAT passed. Restart-during-publication recovery, full provider cleanup proof, and Docker execution remain unverified. Pull request: [#607](https://github.com/gannonh/kata-symphony/pull/607).

## Automated evidence

- `cargo fmt --check` / `cargo clippy -- -D warnings` / `cargo test` — Pass (332 library tests; see [build report](2026-07-29-a3-pr2-build-report.md))
- `cargo llvm-cov --fail-under-lines 72` — Pass
- GitHub backend validation, golden-path smoke, and Kata distribution jobs — Pass
- Branch projection table + bare-remote absent / already-desired / fast-forward / conflict
- Token-authenticated Git subprocesses with timeout and secret redaction
- Pinned forge repository/branch recovery with configuration-drift rejection
- Draft PR create-before-record recovery + foreign/closed/impersonated PR handling
- Persisted draft-PR artifact revalidation and missing-step recovery through post-route finalization
- Bounded-list absence stays retryable; observed PR projection drift stays terminal
- Automatic mode rejects a missing completion route before intent creation
- Run state/events finalize before the publication intent becomes applied
- Retryable issue drift and unexpected forge failures remain visible/recoverable; missing store intent updates error
- Doctor validates the derived publication repository and reports token permissions as unverified
- All 21 inline review threads and four review-summary nitpicks addressed

## Live UAT evidence

The direct Ratatui run used `gannonh/uat-symphony`, user Project #16, and `openai-codex/gpt-5.6-luna:max`:

- Issue [#45](https://github.com/gannonh/uat-symphony/issues/45) received the approved specification, automatic implementation publication, and review preview comment.
- Draft PR [#46](https://github.com/gannonh/uat-symphony/pull/46) was created on `symphony/_45`, remained draft, and used base `main` at `e3a41bc7a833f125e99b77ceb9dd3a7571e1606c`.
- Project Status advanced to `Agent Review` only after draft-PR publication.
- The review preview recorded the reviewed head `59b7e774afe9b96904009015bd205fe033b76868`, produced a structured finding, and created no formal GitHub review or inline comments.
- The TUI rendered typed factory stage sessions, factory counts/completions, issue identifiers, and activity updates during the same end-to-end run.

## Review conclusion

The repaired code preserves the expected-projection/no-force contract and removes the identified stale-evidence, authentication, recovery, secret-handling, route-race, relative-path, and TUI snapshot gaps. Automated evidence and the direct local/GitHub UAT support release cutting for the covered workflow. The remaining residuals are explicit rather than release-blocking for this slice.

## Residual

1. Restart mid-publication recovery on real GitHub
2. Full cleanup proof for issues, labels, states, branches, PRs, containers, workspaces, and blobs
3. Docker daemon profile (if required by the operator UAT matrix)
