---
type: Spec
title: A4 Agent Review Stage
description: Design for turning an owned draft pull request into a structured, read-only agent review that emits schema-validated findings and publishes them deterministically.
tags: [symphony, software-factory, review-stage, github]
timestamp: 2026-07-30T14:45:39Z
status: Migrated
source_status: approved
github_issue: 615
migrated: true
archived_at: 2026-08-04T19:34:07Z
---

> **Migrated to #615.** The GitHub Issue is the canonical spec. This file is history and is not maintained.

# A4 Agent Review Stage

## Status

Active — PR1 review findings preview and typed TUI state are implemented in the current mainline after [#610](https://github.com/gannonh/kata-symphony/pull/610) (`233caf88`). PR2 formal review publication and routing are implemented on `feat/a4-review-publication`; automated gates, active-lease fencing, doctor permission probing, automatic formal-review UAT, and restart-matrix evidence pass. Credential-isolation proof for a live worker and broader Docker evidence remain residuals. See the [PR2 verify report](2026-08-02-a4-pr2-verify-report.md) and [ADR-0005](../../adrs/0005-a4-review-publication-fencing.md).

## Goal

Deliver the fourth software factory stage as a complete user-facing workflow:

`owned draft PR in Agent Review → read-only review worker → schema-validated findings → deterministic PR review publication → routing decision`

A4 implements the PRD's [A4 slice](/specs/archive/symphony-software-factory-platform-prd.md). It consumes the exact draft-PR artifact A3 published, gives a review worker the diff, PR description, approved specification, and repository context but no write credentials, and publishes findings as a single atomic GitHub review that links the issue, specification, factory run, and reviewed head commit.

A4 is **read-only with respect to the change under review**. It never edits code, never pushes, never approves or merges. It produces findings and one routing decision.

## Source of truth

- [Symphony Software Factory Platform PRD, A4](/specs/archive/symphony-software-factory-platform-prd.md)
- [A3 Implementation Stage](2026-07-26-a3-implementation-stage-design.md)
- [ADR-0004 A3 implementation durability and bundles](/adrs/0004-a3-implementation-durability-and-bundles.md)
- [ADR-0005 A4 durable review publication fencing](/adrs/0005-a4-review-publication-fencing.md)
- [A3 PR2 build report](2026-07-29-a3-pr2-build-report.md) / [verify report](2026-07-29-a3-pr2-verify-report.md)
- [A4 PR2 verify report](2026-08-02-a4-pr2-verify-report.md)
- [A2 Spec Stage](2026-07-18-a2-spec-stage-design.md)
- [GitHub REST pull request reviews API](https://docs.github.com/en/rest/pulls/reviews)
- [GitHub REST pull request files API](https://docs.github.com/en/rest/pulls/pulls#list-pull-requests-files)
- `apps/symphony/src/implementation/` (stage, worker boundary, and publication patterns to reuse)
- `apps/symphony/src/github/client.rs`
- `apps/symphony/src/triage/store.rs`

## Product decisions

- A4 admits only factory runs with a stored A3 draft-PR artifact and a tracker item in the configured `Agent Review` state. A tracker state alone is insufficient; the durable artifact is required.
- A review is pinned to one **reviewed head SHA**. Findings are anchored to that SHA and never silently re-anchored.
- The review worker receives the diff, PR description, approved specification, implementation manifest, and read-only repository context. It receives no forge, tracker-helper, SSH, or Git push credentials — the A3 worker boundary applies unchanged.
- The worker emits a typed **review findings manifest**. Unknown fields are rejected. A malformed manifest is a bounded re-prompt, not a partial publish.
- Symphony alone publishes to GitHub, as a **single atomic review** (summary body plus inline comments) rather than N independent comments, so a partial failure cannot leave half a review on the PR.
- Publication is idempotent and restart-safe using the marker and create-before-record recovery A3 established. Every worker-owned durable publication mutation is fenced by a pending status, an active owner lease, and a one-second lease heartbeat during forge and Projects v2 calls. Changed-head supersession claims the same lease before terminalizing the stale cycle; the explicit operator reset path remains separately auditable.
- A new head SHA opens a **new review cycle**. Prior findings are carried forward and classified as resolved, persisting, or new by comparing anchors and finding identity.
- The routing decision is derived from findings, not from the worker's own claim: blocking findings route to the configured changes-requested state; otherwise the item advances toward A5.
- A4 does not apply fixes, approve PRs, run acceptance verification, merge, or deploy. Those remain A5 and A6.
- A4 ships in two vertical pull requests, mirroring A3's shape: review + findings preview, then deterministic review publication and routing.

## Scope

### In scope

- Review-stage eligibility, dispatch ownership, and durable stage attempts (`stage='review'`).
- Read-only worker invocation with diff, spec, manifest, and repository context.
- Review findings manifest schema, validation, and bounded re-prompt on malformed output.
- Durable findings artifacts, immutable per reviewed head SHA.
- Preview publication (comment-only, no PR review, no routing).
- Deterministic atomic review publication with create-before-record recovery.
- Re-review cycles on head-SHA change, with finding carry-forward classification.
- Routing decision and tracker state transition.
- Config, doctor validation, HTTP status exposure, events, and metrics.

### Out of scope

- Applying suggested remediations or pushing any commit.
- Approving or requesting changes as a formal GitHub approval decision that gates merge.
- Acceptance verification and evidence (A5), merge and deployment governance (A6).
- Linear review publication. GitHub only, matching A1/A3.
- Multi-reviewer consensus or reviewer-model ensembles.

## Eligibility and dispatch ownership

A run is A4-eligible when all hold:

1. A terminal A3 publication intent exists (`applied`) with a stored draft-PR artifact.
2. The tracker item is in the configured `review.trigger_state` (default `Agent Review`).
3. The live PR is open and its head SHA has no completed review cycle for that SHA.
4. No non-terminal A4 attempt is already claimed for the run.

The A4 coordinator claims eligible work before any legacy path, using the same durable dispatch guard A3 uses. A4 and A3 cannot own the same run simultaneously: A3's intent must be terminal first.

## Stage attempt model

Reuse `stage_runs` with `stage='review'`. Each attempt records inputs (reviewed head SHA, spec artifact id, draft-PR artifact id), the worker turn, the emitted manifest, validation result, and outcome. Attempts are durable across restart.

## Worker boundary

Identical trust boundary to A3, with one narrowing: A4's worker has no reason to write to the repository at all, so its workspace is mounted read-only where the execution profile supports it.

The worker receives:

- the unified diff for the reviewed head SHA against the PR base,
- the PR description,
- the byte-exact approved specification file,
- the A3 implementation manifest (including its acceptance-criterion mapping),
- read-only repository context for files the diff touches.

The worker never receives forge tokens, tracker helper credentials, SSH keys, or push URLs.

## Review findings manifest

A typed document emitted by the worker and validated by Symphony before anything is published. Unknown fields are rejected.

Each finding carries:

| Field | Purpose |
| --- | --- |
| `finding_id` | Worker-assigned, stable within the manifest; Symphony derives the durable identity |
| `severity` | `blocking`, `major`, `minor`, `nit` |
| `category` | e.g. `correctness`, `security`, `spec-conformance`, `test-coverage`, `maintainability` |
| `path` | Repository path, must be present in the reviewed diff |
| `line` / `end_line` | Anchor within the reviewed head SHA |
| `claim` | One-sentence statement of the defect |
| `rationale` | Why it is a defect, referencing the diff or spec |
| `remediation` | Suggested fix as text; never applied by A4 |
| `acceptance_criterion` | Optional link back to an approved-spec criterion |
| `confidence` | Worker's confidence, recorded for measurement, never used to suppress |

Manifest-level fields record the reviewed head SHA, the base SHA, a spec-conformance summary, and an explicit `no_findings` affirmation when the manifest is empty — so "found nothing" is distinguishable from "failed to review."

**Validation rules:** every `path` must appear in the reviewed diff; every anchor must resolve within that file at the reviewed SHA and fit wholly within one valid right-side diff range (changed or context lines accepted by GitHub); severities and categories must be in the closed vocabulary; a non-empty manifest must not set `no_findings`. A violation is a bounded re-prompt with structured feedback, capped, then blocked with an actionable error.

## Durable findings storage

Migration `006_review_stage.sql` adds:

- `review_attempts` — stage attempt inputs and outcome.
- `review_findings_artifacts` — immutable manifest per (run, reviewed head SHA), unique on that pair.
- `review_publication_intents` — publication intent mirroring the A3 shape, with progressive completed steps.
- `review_finding_records` — per-finding durable identity, anchor, and lifecycle state across cycles.

All additive. No A1/A2/A3 table is altered.

## Publication

### Preview (PR1)

An owned issue comment marked `<!-- symphony:review:{intent_id} -->` renders the findings summary. No PR review is created, no inline comment is posted, no tracker state changes. Idempotent via marker upsert.

### Atomic review (PR2)

Symphony creates **one** GitHub review containing the summary body and all inline comments, so the PR never shows a partially published review. The review body carries the marker and records the issue, spec version, factory run, and reviewed head SHA.

Create-before-record recovery follows the A3 contract exactly: before creating, list existing reviews for the reviewed SHA and adopt an owned, marker-matching, author-matching review if one exists. Adoption requires the authenticated publisher to be the author. A foreign or drifted review is a terminal conflict, never silently replaced.

### Publication steps

Progressive steps are recorded durably before the intent is terminalized, matching A3: `review_created` → `findings_recorded` → `route_applied` → `comment_final`. Restart resumes at the first incomplete step.

## Re-review cycles

When the PR head SHA changes, a new cycle opens against the new SHA. Symphony classifies each prior finding as:

- **resolved** — its anchor is gone or the flagged construct no longer appears in the diff,
- **persisting** — it still anchors at the new SHA,
- **new** — first seen in this cycle.

Persisting findings are not re-posted as fresh inline comments; the summary reports their continued state. This is what makes re-review contextual rather than repetitive.

## Retry, waiting, and terminal states

This section encodes what [#607](https://github.com/gannonh/kata-symphony/pull/607) cost to learn. A4 adopts it from the start rather than rediscovering it in review.

- Reconcile attempts are bounded with exponential backoff and a retry ceiling that terminalizes as `blocked`.
- **The budget counts failed attempts only.** A condition waiting on an unmet precondition — the PR being temporarily unavailable, a head SHA that has moved and awaits a fresh cycle, a human still editing — is recorded as a *waiting* state that does not charge the budget. Charging a human-paced wait to a failure budget strands work permanently.
- Terminal `blocked` intents **must have a documented operator recovery path**. A3's landed first, as `symphony publication list-blocked` / `symphony publication reset <intent-id>`; A4 extends the same command to review intents rather than inventing a second mechanism.
- Error deduplication must compare like with like. `SymphonyError` variants prefix their payload on `Display`; comparing that rendered string against a raw `FactoryError::remediation` never matches, silently double-charging the retry budget and clobbering specific error codes with a generic one.

## Configuration

A `review` section in `ServiceConfig`:

```
[review]
mode = "preview" | "automatic"
trigger_state = "Agent Review"
completion_route = { state = "Human Review" }
changes_requested_route = { state = "Implementation" }
blocking_severity = "blocking"
max_findings = 50
max_reprompts = 2
```

`doctor` validates: trigger and completion states exist on the board, the token can create PR reviews on the target repository, `blocking_severity` is in the vocabulary, and automatic mode has both routes configured — fail-fast before a durable intent is created, the correction #607 landed late.

## HTTP API and events

Run attach exposes the current review cycle, reviewed head SHA, finding counts by severity, publication status, and the review URL. Events: `review_started`, `review_findings_recorded`, `review_published`, `review_blocked`, `review_cycle_reopened`.

## Acceptance criteria

1. An eligible run in `Agent Review` with a stored draft-PR artifact is claimed by A4 exactly once.
2. A run without a terminal A3 publication is never claimed.
3. The worker receives no forge, tracker, SSH, or push credentials, asserted by test.
4. A manifest with unknown fields is rejected.
5. A finding whose `path` is absent from the reviewed diff is rejected.
6. A finding whose anchor does not resolve at the reviewed SHA is rejected.
7. An empty manifest without `no_findings` is rejected; with it, it publishes a clean review.
8. A malformed manifest re-prompts up to `max_reprompts`, then blocks with an actionable error.
9. Preview mode creates no PR review, no inline comment, and no tracker change.
10. Automatic mode publishes exactly one review containing summary and all inline comments.
11. Restart between review creation and record adopts the existing owned review rather than creating a second.
12. A review authored by another identity is a terminal conflict, never replaced.
13. Publication steps are durable; restart resumes at the first incomplete step.
14. Blocking findings route to `changes_requested_route`; otherwise `completion_route`.
15. A head-SHA change opens a new cycle and classifies prior findings as resolved, persisting, or new.
16. Persisting findings are not duplicated as fresh inline comments.
17. A waiting condition does not consume the retry budget.
18. A failed attempt does consume it; the ceiling terminalizes as `blocked` with a non-retryable error.
19. A `blocked` review intent is recoverable through the documented operator path.
20. A4 never pushes a commit, never approves, and never merges, asserted by test.

## Measures

Accepted finding rate, dismissed finding rate, review cycles per PR, escaped sampled defects, false-positive rate by category, time-to-first-review, and review cost per run.

## Testing strategy

Unit: manifest schema validation, anchor resolution, finding identity and carry-forward classification, severity routing, retry/waiting budget behavior, marker parsing.

Integration: full preview path against a fake forge; automatic path with create-before-record recovery, restart at each publication step, foreign-review conflict, and re-review across a head-SHA change.

Quality gate: `cargo clippy -- -D warnings`, `cargo fmt --check`, full test suite, and the existing coverage floor.

## Likely file map

```
apps/symphony/src/review/{mod,domain,coordinator,worker,manifest,findings,publisher,automatic}.rs
apps/symphony/src/triage/migrations/006_review_stage.sql
apps/symphony/src/github/client.rs         # + list_pull_request_files, get_diff, create_review, list_reviews
apps/symphony/src/triage/{store,runtime}.rs
apps/symphony/src/{doctor,http_server}.rs
```

`github/client.rs` today has issue comments, labels, PR listing, and PR creation. A4 adds diff/files retrieval and the reviews API — the largest net-new forge surface in this slice.

## Delivery slices

**PR1 — review stage and findings preview.** **Implemented** in [#610](https://github.com/gannonh/kata-symphony/pull/610) (`233caf88`): eligibility, dispatch ownership, stage attempts, worker invocation and boundary, manifest schema and validation, bounded re-prompt, durable findings artifacts, preview comment, HTTP/events, and typed TUI state. No PR review, no routing.

**PR2 — deterministic review publication and routing.** **Implemented on `feat/a4-review-publication`; verified with residuals.** Atomic review creation, create-before-record recovery, active-lease fencing, progressive publication steps, routing decision, re-review cycles and carry-forward, retry/waiting/terminal semantics, doctor validation, and operator recovery path are present. Live formal UAT and restart-matrix evidence pass; live worker credential-isolation proof and broader Docker evidence remain residuals.

## Risks and mitigations

### Residual risk from provider-side operation windows

Durable publication writes are fenced by active leases and the publisher renews its lease during forge and Projects v2 calls. A provider request already accepted before a process failure cannot be cancelled by SQLite; marker ownership, live-head validation, reconciliation, and idempotent route writes handle recovery. A live worker credential-isolation proof and broader Docker execution evidence remain unverified. A4 continues to treat malformed or unresolvable draft-PR artifacts as explicit blocked states with actionable errors.

### No operator recovery from `blocked`

**Resolved before A4 PR1.** A3 shipped with terminal `blocked` intents and no code path out; clearing one required direct SQLite edits. `symphony publication list-blocked` and `symphony publication reset <intent-id>` now close that, preserving completed steps so publication resumes, and recording the intervention on the run timeline. Both commands go through the orchestrator's admin HTTP surface (`GET /api/v1/publications/blocked`, `POST /api/v1/publications/{intent_id}/reset?operator=`) and fall back to the durable store only when nothing answers — the store's exclusive lock is held while Symphony runs, which is precisely when recovery is needed. A4 extends the same command surface and the same HTTP-first shape to review intents; it must not introduce a terminal state without one, nor a recovery command that only works while Symphony is stopped.

### Review findings are wrong or noisy

An agent reviewer that flags non-defects trains humans to ignore it. Mitigation: measure accepted/dismissed rate by category from the first cycle; require `rationale` to reference the diff or spec; never suppress by confidence, so the data stays honest.

### Anchors drift as the PR moves

Inline comments anchored to a stale SHA land on the wrong lines. Mitigation: pin every cycle to one reviewed head SHA, validate anchors at that SHA before publishing, and open a new cycle rather than re-anchoring.

### Review cost grows with diff size

Large diffs are expensive and produce worse reviews. Mitigation: `max_findings`, and a diff-size threshold above which the stage blocks with a "split this change" finding rather than reviewing badly.

## Explicitly deferred work

Linear review publication, formal GitHub approve/request-changes decisions, reviewer ensembles, applying remediations, and any merge gating.
