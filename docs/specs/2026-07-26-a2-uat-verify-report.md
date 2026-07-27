---
type: Verify Report
title: A2 Spec Stage UAT Verify Report
status: Accepted
description: Live GitHub UAT and post-UAT remediation results for the A2 spec stage against acceptance criteria 15–16 and criterion 11 metrics/token measures.
tags: [symphony, spec-stage, a2, verify, uat]
timestamp: 2026-07-26T21:18:00Z
---

# A2 Spec Stage — UAT Verify Report

## Status

**Accepted** — live GitHub UAT on the dedicated UAT project plus automated suite both Pass after nine product defects and two measure gaps found during UAT were fixed and re-verified.

## Spec / Implementation

- Spec: [`2026-07-18-a2-spec-stage-design.md`](2026-07-18-a2-spec-stage-design.md)
- ADR: [ADR-0003 A2 spec-stage artifacts and human gates](/adrs/0003-a2-spec-stage-artifacts-and-gates.md)
- Implementation commits on `katacode/implement-a2`:
  - `1dfe7b4c` — initial A2 stage
  - `4fda65a1` — nine live-UAT product defects
  - `17999528` — criterion 11 metrics and Pi token capture

## Environments

| Layer | Target |
| --- | --- |
| Automated | `cargo test --lib` (256), HTTP metrics tests, `cargo fmt --check`, `cargo clippy -- -D warnings` |
| Live UAT | `/Volumes/EVO/dev/uat-runs/kata-symphony` → `gannonh/uat-symphony` Project [#16](https://github.com/users/gannonh/projects/16) |
| Model | `openai-codex/gpt-5.6-luna:high` (draft + review) |

## Results

| AC / measure | Result | Notes |
| --- | --- | --- |
| 15 (PR1 preview UAT) | Pass | Versioned owned spec comment; off-project diagnostic idempotent; dual intake label skip with no spec comment; labels unchanged in preview |
| 16 (PR2 decision UAT) | Pass | `spec-revise` + feedback → version 2; `spec-approved` → `ready-for-agent` + Todo, intake/decision labels removed, `approved_version` pinned; restart during pending approval completed idempotently |
| 11 metrics (`stage=spec`) | Pass | Live response includes attempt/failure/ineligible/duration/tokens plus `review_cycles`, `converged_attempts`, `convergence_rate`, `revision_requests`, `approval_latency` |
| Token measures | Pass | Pi `message_end` usage captured per turn; #31 draft/review non-zero; metrics `tokens_by_harness_model` populated |
| Automated gate | Pass | 256 lib tests; HTTP stage=spec payload; store aggregate and runner usage regressions |

## Live fixtures

- [#28](https://github.com/gannonh/uat-symphony/issues/28) — full journey: v1 → revise+feedback → v2 → approve → implement route
- [#29](https://github.com/gannonh/uat-symphony/issues/29) — off-project: one diagnostic comment/intent/event across many polls
- [#30](https://github.com/gannonh/uat-symphony/issues/30) — dual intake: one `spec_ineligible` (`intake_label_conflict`), no spec comment
- [#31](https://github.com/gannonh/uat-symphony/issues/31) — token smoke after capture fix (closed after evidence)

UAT repo notes: `A2-UAT.md` in `gannonh/uat-symphony`.

## Defects found and fixed during UAT

Product path (`4fda65a1`):

1. Relative `workspace.root` / `workspace.repo` rejected
2. Post-claim failures left nonterminal attempts
3. Issue `updated_at` in fingerprint caused publication loops
4. Diagnostic intent/event growth every poll
5. Feedback cutoff advanced by diagnostic republish
6. Author-only Symphony comment matching discarded maintainer feedback under `gh auth token`
7. Agent retries consumed human revision budget
8. Unconditional label removal 404 aborted approval
9. Reconcile path omitted `spec_route_applied` / `spec_approved`; spec poll failures surfaced as triage failures

Measure path (`17999528`):

- Spec metrics omitted review-cycle, convergence, revision-request, approval-latency aggregates
- Pi harness always recorded zero tokens

## Caveats

- Historical turns recorded before the token fix remain zero; new turns capture usage.
- A1 PR3 subsequently shipped in [#599](https://github.com/gannonh/kata-symphony/pull/599).
- Documented A2 narrowings stand: tracker-only approval; one artifact with product and technical sections.

## Recommendation

**Accepted.** A2 GitHub tracker workflow is complete for the PRD demo path. Next factory slice is A3 consumption of the pinned approved artifact.
