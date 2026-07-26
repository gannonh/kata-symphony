---
type: Spec
title: A2 Spec Stage
status: Completed
description: Design for a durable spec stage that turns spec-routed GitHub issues into reviewed, human-approved specification artifacts and implementation-ready runs.
tags: [symphony, software-factory, spec-stage, github]
timestamp: 2026-07-18T17:24:00Z
---

# A2 Spec Stage

## Status

Completed — GitHub tracker workflow implemented and live UAT accepted ([verify report](/specs/2026-07-26-a2-uat-verify-report.md); [ADR-0003](/adrs/0003-a2-spec-stage-artifacts-and-gates.md)).

## Goal

Deliver the second software factory stage as a complete user-facing workflow:

`GitHub issue labeled ready-to-spec → repository-backed draft → adversarial review loop → published versioned spec → human approval or revision → implementation-ready run`

A2 implements the PRD's A2 slice: a spec-routed issue produces versioned product behavior, technical approach, acceptance criteria, and open decisions. A human approves or requests revision from the tracker. Approval makes the run implementation-ready by applying the configured implement route.

A2 reuses A1's durable factory-run store, isolated one-turn runner, publisher-owned comments, expected-projection publication intents, and intake patterns. It adds a multi-invocation pipeline (draft → review → revise), a versioned spec artifact, and a label-driven human decision loop.

## Source of truth

- [Symphony Software Factory Platform PRD, A2](/specs/symphony-software-factory-platform-prd.md)
- [A1 GitHub Issue Triage](/specs/2026-07-16-a1-github-issue-triage-design.md)
- [ADR-0001 A1 triage durability and isolation](/adrs/0001-a1-triage-durability-and-isolation.md)
- [Warp spec-driven development article](https://www.warp.dev/blog/how-to-build-a-cloud-software-factory-add-spec-driven-development-skills)
- `apps/symphony/src/triage/` (store, runner, publisher, intake, coordinator, fingerprint, integrity)
- `apps/symphony/src/domain.rs`, `apps/symphony/src/config.rs`, `apps/symphony/src/http_server.rs`

## Product decisions

- The spec stage triggers on the configured spec-route label (default `ready-to-spec`) on open, project-member GitHub issues, regardless of whether a human or the A1 publisher applied the label. A2 does not depend on A1 PR2 shipping first.
- One spec artifact per version with structured sections: product behavior, technical approach, acceptance criteria, open decisions. No separate product/technical artifacts.
- The spec pipeline runs draft → adversarial review → revise as separate isolated one-turn runner invocations inside one stage attempt. Every turn uses a fresh, turn-unique workspace and stage input directory containing only the allowlisted inputs for that turn kind. The reviewer receives no draft conversation context.
- "Version" refers only to published spec artifacts. One completed attempt publishes exactly one version; in-attempt revise outputs are turn outputs, not versions.
- The review → revise loop iterates until the reviewer emits zero blocking findings or `spec.max_review_cycles` is reached. At the cap, the spec publishes anyway with unresolved blocking findings surfaced in open decisions and the published comment. The human approval gate is the backstop.
- The reviewer model is configurable via `spec.review_model` and defaults to the drafter's resolved model.
- The spec publishes as one publisher-owned marked issue comment. Humans decide via labels: `spec-approved` or `spec-revise` plus ordinary feedback comments.
- Approval applies the configured implement route (label plus optional Projects v2 state) through a deterministic publication intent and pins the approved artifact version on the factory run. A1's tracker-state handoff boundary to the implementation scheduler is unchanged.
- Revision requests start a new bounded spec attempt seeded with the prior spec and human feedback, producing version N+1 in the same owned comment.
- A2 ships as two vertical pull requests: spec generation preview (read-only), then approval/revision loop with implement handoff.
- A2 intentionally narrows the PRD A2 slice: approval is tracker-only (Pi approval belongs to Horizon B surfaces), and one artifact with product and technical sections satisfies the PRD's "both artifacts" demo. Record these narrowings on the PRD when A2 ships.
- Linear, spec PRs in the repository, HTTP approval endpoints, and multi-turn continuation within one invocation are out of scope.

## User stories

### Maintainer

As a maintainer, I can label an issue `ready-to-spec` and receive a reviewed, versioned specification on the issue. I approve it or request one revision with a comment, and the approved issue becomes implementation-ready without further manual routing.

### Factory operator

As a factory operator, I can inspect every draft, review-finding, and revision artifact durably, diagnose pipeline failures, restart Symphony safely during any publication, and see whether approval effects completed.

### Engineering leader

As an engineering leader, I can measure approval cycles, time awaiting human input, review-loop convergence, and token usage per approved spec from durable records.

## Current state

Shipped in A1 PR1 ([#587](https://github.com/gannonh/kata-symphony/pull/587)):

- SQLite factory-run store with runs, stage attempts, immutable artifacts, publication intents, durable events, exclusive locking, leases, and migrations (`triage/store.rs`, `triage/migrations/`);
- label-based Projects v2 intake with pagination-to-exhaustion and ineligible diagnostics (`triage/intake.rs`);
- isolated one-turn Pi/Codex runner with cleared environment, isolated home, clone-only workspace, integrity checks, and `SYMPHONY_STAGE_OUTPUT` transport (`triage/runner.rs`, `triage/integrity.rs`);
- publisher-owned marked comments with comment-ID and author verification, create-before-record recovery, and idempotent reconciliation (`triage/publisher.rs`, `triage/comment.rs`);
- canonical issue-revision and configuration-revision fingerprints (`triage/fingerprint.rs`);
- factory-run HTTP read API and durable/live triage events.

A2 must add:

- a second stage type in the factory-run model with multi-invocation attempts;
- spec and review-findings artifact schemas with versioning;
- a pipeline coordinator sequencing draft, review, and revise invocations;
- reviewer model resolution independent of the drafter;
- a spec comment renderer and decision-label detection;
- approval publication (implement route + pinned artifact version) and revision attempts;
- stale-approval and conflict handling across issue and configuration revisions;
- spec HTTP surface, events, doctor checks, prompts, and starter configuration.

Not yet shipped and not required by A2: A1 PR2 automatic route publication, A1 PR3 correction measurement. When A1 PR2 ships, its `spec` route applies the same label A2 polls; no A2 change is needed.

## Scope

### In scope

- GitHub Issues intake by spec-route label with Projects v2 membership, reusing A1 intake behavior.
- Draft → review → revise pipeline of isolated one-turn invocations with a bounded review loop.
- Versioned, immutable spec and findings artifacts in the existing SQLite store.
- Publisher-owned spec comment with all sections, version, unresolved findings, and review instructions.
- `spec-approved` / `spec-revise` decision-label detection on the poll cadence.
- Bounded human revision loop seeded with prior spec and feedback comments.
- Approval publication: pin approved version, apply implement label and optional Projects v2 state, remove spec-route and decision labels.
- Stale-approval detection across issue-revision and configuration-revision changes.
- Factory-run HTTP additions, durable and live spec events, doctor checks, starter prompts and configuration, automated tests, and GitHub UAT.

### Out of scope

- Linear spec stage.
- Triggering only from durable triage artifacts (label intake covers both manual and A1-published routes).
- Spec artifacts as repository files or spec PRs.
- HTTP or chat approval surfaces; the tracker is the only decision surface (Horizon B owns authenticated remote control).
- Multi-turn continuation, steering, or escalation within a single runner invocation.
- A3 implementation consumption of the approved spec; A2 only guarantees a pinned approved artifact version exists on the run.
- Confidence- or risk-based pipeline shaping.
- Automatic label creation.
- Cost-in-currency attribution; token usage only.

## Spec pipeline model

### Stage attempt

One spec stage attempt owns an ordered sequence of turn records:

1. **Draft turn** produces the initial spec content.
2. **Review turn** consumes the latest spec content and produces a findings artifact.
3. **Revise turn** consumes the latest spec content plus blocking findings and produces replacement spec content.

After each review turn:

- zero blocking findings → the attempt completes with the latest spec content;
- blocking findings and another cycle available → run a revise turn, then review again;
- blocking findings at `spec.max_review_cycles` → the attempt completes with the latest spec content plus the deterministic cap post-processing below.

Cap post-processing is performed by the coordinator, not the agent: it copies the latest spec content, appends one bounded summary line per unresolved blocking finding to `open_decisions` (oldest first), drops appended lines beyond the 50-entry array cap, re-validates the result once, and stores that as the immutable published artifact. All unresolved blocking findings render in the published comment even when they do not all fit in `open_decisions`.

Cycle counting: one cycle is one review turn. Default `spec.max_review_cycles: 3` yields at most draft + 3 reviews + 2 revises = 6 invocations per attempt.

Each turn is a separate isolated runner invocation with a fresh turn-unique clone workspace, isolated home, and stage input directory containing only the allowlisted inputs for its turn kind, plus its own timeout and usage record. Integrity checks run after every turn. Workspace reuse across turns is prohibited: it would leak draft-turn files into the reviewer's context. Any turn failure (process, timeout, schema, integrity) fails the whole attempt; retry follows A1 semantics with a new attempt number bounded by `spec.max_attempts`.

The reviewer invocation receives the issue context and the current spec content only. It does not receive the draft conversation, prior findings, or revision history.

### Spec artifact schema

UTF-8 JSON, schema version 1, written to `SYMPHONY_STAGE_OUTPUT`, 64 KiB file cap, unknown fields rejected, empty-after-trim strings invalid. Symphony records all timestamps.

```json
{
  "schema_version": 1,
  "product_behavior": "Markdown describing user-facing behavior, workflows, and outcomes.",
  "technical_approach": "Markdown describing architecture, affected areas, and sequencing.",
  "acceptance_criteria": [
    "Labeling a project issue ready-to-spec produces a published spec comment."
  ],
  "open_decisions": [
    "Should the intake poll share the triage cadence or use its own interval?"
  ]
}
```

Contract:

- `schema_version`: integer, exactly `1`.
- `product_behavior`: non-empty string, at most 16,000 UTF-8 bytes.
- `technical_approach`: non-empty string, at most 16,000 UTF-8 bytes.
- `acceptance_criteria`: array of 1 to 50 non-empty strings, each at most 1,000 UTF-8 bytes.
- `open_decisions`: array of 0 to 50 non-empty strings, each at most 1,000 UTF-8 bytes.

### Review findings schema

Same transport and validation rules.

```json
{
  "schema_version": 1,
  "verdict": "revise",
  "findings": [
    {
      "severity": "blocking",
      "section": "acceptance_criteria",
      "summary": "Criterion 3 is not observable.",
      "recommendation": "State the exact API response or label change that proves completion."
    }
  ]
}
```

Contract:

- `schema_version`: integer, exactly `1`.
- `verdict`: `pass` or `revise`.
- `findings`: array of 0 to 30 objects. `verdict: pass` requires zero `blocking` findings; `verdict: revise` requires at least one `blocking` finding. A mismatch fails validation.
- `findings[].severity`: `blocking` or `advisory`.
- `findings[].section`: one of `product_behavior`, `technical_approach`, `acceptance_criteria`, `open_decisions`, `general`.
- `findings[].summary`: non-empty string, at most 1,000 UTF-8 bytes.
- `findings[].recommendation`: non-empty string, at most 1,000 UTF-8 bytes.

### Spec versions

One completed attempt stores exactly one immutable published spec artifact. Published versions are numbered monotonically per factory run: the first completed attempt publishes version 1, and each later completed attempt (from a human revision request or a new revision pair) publishes `max(existing versions) + 1`. In-attempt revise turns never increment the version; their outputs, along with every findings payload, are stored durably against the attempt as turn records for inspection.

## Configuration

A2 adds a `spec` section to `WORKFLOW.md`, following A1's storage and triage conventions. Exact Rust type names follow repository conventions; user-facing concepts are fixed by this spec.

```yaml
spec:
  enabled: true
  intake_label: ready-to-spec
  prompts:
    draft: prompts/spec-draft.md
    review: prompts/spec-review.md
    revise: prompts/spec-revise.md
  model: anthropic/claude-sonnet-4-6
  review_model: anthropic/claude-sonnet-4-6
  turn_timeout_ms: 1800000
  max_intake_pages: 100
  max_review_cycles: 3
  max_attempts: 3
  max_revision_requests: 3
  labels:
    approved: spec-approved
    revise: spec-revise
  approval_route:
    label: ready-for-agent
    state: Todo
```

### Configuration behavior

- `spec.enabled` defaults to `false`.
- `spec.intake_label` defaults to `ready-to-spec`. When triage is enabled, it must equal the triage `spec` route label so A1 publication and A2 intake compose; doctor reports a mismatch.
- The three prompts resolve relative to the active `WORKFLOW.md` directory and are all required when spec is enabled.
- Pi model precedence: `spec.model`, then `agent.model`, then harness default. `spec.review_model` overrides the reviewer only and defaults to the drafter's resolved model. Config validation rejects `spec.model` and `spec.review_model` when `agent.name` is `codex`, matching A1's Codex contract.
- `spec.turn_timeout_ms` applies per invocation, defaults to `1800000`, and must be greater than zero.
- `spec.max_intake_pages` defaults to `100`, must be greater than zero, and applies A1's cap semantics to every completeness-sensitive spec-stage GitHub read (intake, membership, comments, marked-comment recovery): reaching the cap fails the whole operation visibly with no attempts from partial results.
- `spec.max_review_cycles` defaults to `3`, must be greater than zero.
- `spec.max_attempts` defaults to `3`, must be greater than zero, and bounds agent-failure retries per `(issue_revision, configuration_revision)` exactly as in A1.
- `spec.max_revision_requests` defaults to `3`, must be greater than zero, and bounds human-requested revision attempts per factory run.
- `spec.labels.approved` and `spec.labels.revise` are required, distinct, and cannot equal the spec intake label or the approval-route label. When triage is configured, they also cannot equal the triage intake label or any configured triage route label; when triage is absent, only the spec-managed set applies.
- `spec.approval_route.label` is required when spec is enabled; `state` is optional and resolves through the configured Projects v2 status field. Starter configuration uses `ready-for-agent` and `Todo`, matching A1's implement route.
- The spec configuration revision hashes schema versions, all three prompt contents, models, timeout, cycle and attempt bounds, decision labels, and the approval route mapping.
- Storage reuses the A1 `storage` section, database, exclusive lock, and lease rules. Spec and triage stages share one store per repository.
- `symphony doctor` validates prompts, intake label, decision labels, approval-route label and state, Projects v2 access, isolated harness authentication, and the triage-route consistency check above. `symphony init` writes the three starter prompts and commented starter configuration; it creates no remote labels.
- Disabling spec stops new intake and decision detection but does not cancel durable attempts or publication reconciliation.

## Architecture

### Intake

A spec intake port reuses A1's intake mechanics against `spec.intake_label`: open issues, pagination to exhaustion under the existing page cap, Projects v2 membership by repository and issue number, and one idempotent publisher-owned diagnostic comment for off-project issues (ineligible run record, `spec_ineligible` event, no agent attempt, label unchanged).

An issue carrying both the triage intake label and the spec intake label is skipped by spec intake until `needs-triage` is absent: triage takes precedence. The skip emits one durable `spec_ineligible` event with error code `intake_label_conflict` per `(issue, issue_revision)`, records or updates an ineligible-run note, and posts no comment (the triage flow owns the issue's comment surface).

The normalized input matches A1's shape (title, body, non-managed labels, assignee/milestone, conversation excluding verified Symphony comments, timestamps, forge and repository identity) plus, when a durable triage artifact with route `spec` exists for the issue, that artifact's rationale, evidence, and next action as advisory context.

### Issue revision and configuration revision

The A1 canonical fingerprint applies with these managed-content extensions:

- excluded labels additionally include the spec intake label, `spec-approved`, `spec-revise`, and the approval-route label;
- excluded comments additionally include verified publisher-owned spec and diagnostic comments;
- human feedback comments are included, so a `spec-revise` feedback comment produces a new issue revision, which is what a revision attempt keys on.

At most one nonterminal spec attempt and one successful spec artifact exist per `(stage, issue_revision, configuration_revision)`, with terminal failed attempts retained up to `spec.max_attempts`. This extends A1's per-revision constraints with stage scoping so triage and spec attempts on the same factory run cannot collide.

### Store schema extension

The shipped A1 schema is triage-scoped: the nonterminal-attempt unique index omits `stage`, the artifact table is triage-shaped with per-run revision uniqueness, attempt claiming hardcodes the triage stage name, and no turn-record, version, or pinning columns exist. A2 PR1 therefore includes a store extension behind the existing storage interface:

- stage-scoped nonterminal uniqueness on `(run_id, stage, issue_revision, configuration_revision)`;
- stage-parameterized attempt claiming;
- a spec artifact table (or stage-typed artifact generalization) with per-run monotonic `version` and success uniqueness per `(stage, issue_revision, configuration_revision)`;
- a turn-record table keyed by stage run with turn kind, ordinal, status, timing, usage, model, and output reference;
- factory-run columns or a table for `approved_version` / `approved_artifact_id`, plus a revision-request counter;
- publication-intent generalization for spec preview, approval, and diagnostic modes.

Migrations are additive; the complete existing triage test suite must pass unchanged after the extension. This extraction is a required part of PR1, not an optional refactor.

### Spec coordinator

A spec coordinator, separate from the triage coordinator but sharing the store, lock, lease, and reconciliation infrastructure, runs on the poll cadence:

1. reconcile pending spec publication and diagnostic intents;
2. detect decision labels on issues with published specs (PR2);
3. fetch and fingerprint spec intake;
4. claim or skip each revision transactionally;
5. run the draft → review → revise pipeline;
6. validate outputs and repository cleanliness after every turn;
7. store turn records, findings, and the completed spec artifact;
8. create or update the spec publication intent;
9. emit durable and live events.

Startup ordering follows A1: lock and migrate, recover publisher-owned comment identities, reconcile pending intents, then fingerprint intake.

A multi-invocation attempt must not starve the poll loop: between turns the coordinator yields so pending publication intents, decision detection, and triage reconciliation continue to progress while a long pipeline runs (default bounds allow up to six 30-minute turns per attempt). Build chooses the mechanism (background attempt task or turn-boundary re-entry); tests prove a pending intent reconciles while a multi-turn attempt is in flight.

### Runner reuse

Each pipeline turn invokes the existing isolated one-turn runner with a spec-specific process profile: clone-only workspace, cleared environment, isolated home, no forge or helper credentials, no lifecycle hooks, own process group, `SYMPHONY_STAGE_OUTPUT` transport, and A1's integrity contract (unchanged `HEAD` and submodules, no staged, unstaged, or untracked source entries) checked after every turn.

Turn inputs are provided as files in the stage input directory referenced by the prompt: the normalized issue context for all turns; the current spec JSON for review and revise turns; the blocking findings JSON for revise turns; prior spec version and human feedback comments for revision attempts. Model output prose never substitutes for the output file.

### Spec comment and publication

The spec publisher reuses A1's contract: intent-marked comment `<!-- symphony:spec:{intent_id} -->`, stored comment ID and authenticated publisher login, author verification, create-before-record pagination recovery, expected-projection steps, and bounded errors.

The published spec comment renders:

- spec version and factory run / attempt IDs;
- product behavior, technical approach, acceptance criteria, open decisions;
- unresolved blocking findings when the review loop hit its cap;
- review instructions: apply `spec-approved` to approve, or apply `spec-revise` and add a feedback comment to request changes;
- in PR1, a preview notice stating that decision labels are not yet acted on.

Repeated publication updates the same owned comment ID. New versions update the comment in place; prior versions remain inspectable through the API.

### Decision detection and approval publication (PR2)

On each poll, for every factory run with a published, undecided spec version, the coordinator reads current labels and evaluates exactly one branch of this ordered decision table. Decision handling runs before intake fingerprinting each poll, and intake never claims a revision pair that decision handling has already claimed in the same poll; the store's stage-scoped nonterminal uniqueness enforces single ownership across polls.

1. **Both decision labels present** → conflict diagnostic on the owned comment; no attempt, no publication, no other branch evaluated until a human removes one label.
2. **`spec-revise` present with feedback** → start one seeded revision attempt (bounded by `spec.max_revision_requests`) for the new revision pair. Feedback is any non-Symphony comment whose `created_at` or `updated_at` is later than the owned spec comment's last publication time; tests cover an edited pre-publication comment counting as feedback. The completed revision republishes, and the publisher removes `spec-revise` as part of republication. The stale branch does not apply: the feedback-driven revision-pair change is claimed by the seeded attempt.
3. **`spec-revise` present without feedback** → diagnostic on the owned comment asking for feedback; no attempt starts.
4. **`spec-approved` present** and the run's recorded issue revision and configuration revision match current state → create an approval publication intent.
5. **`spec-approved` present on a stale revision pair** → record a `spec_publication_conflict` event and a diagnostic on the owned comment; no publication and no automatic new attempt. A new attempt requires the intake label plus the changed revision pair through ordinary intake, keeping approval intent strictly human-resolved.
6. **No decision label, revision pair changed, intake label present** → ordinary intake starts a cold (unseeded) attempt for the new pair.

Revision-attempt seeding: a seeded attempt skips the draft turn and begins with a revise turn whose input set is the prior published spec version plus the qualifying feedback comments, then enters the normal review loop. A cold attempt starts from the draft turn with no prior spec input.

Approval publication intent steps:

1. update the owned comment to `approval: pending`, naming the version and recording it durably as `pending_approval_version`;
2. remove the spec intake label and both decision labels;
3. apply `spec.approval_route.label`;
4. apply the optional Projects v2 state;
5. pin `approved_version` / `approved_artifact_id` on the factory run;
6. update the owned comment to `approved — implementation-ready`, recording version, label, and state.

The run reports `spec-approved`/implementation-ready over HTTP and events only after step 6; until then the API exposes `pending_approval_version` and a pending publication. `approved_version` is set only at step 5, so a blocked or conflicted intent never leaves a pin without a decision. Each step uses A1's expected-projection reconciliation: observed state equal to expected proceeds, equal to desired records prior success, different from both stops with a human conflict. Crash windows between GitHub success and local recording reconcile without duplication.

**Implementation dispatch guard.** The implement label and state (steps 3–4) are applied before the intent is final, so the existing implementation scheduler must reject every issue ID that has a nonterminal spec approval publication intent, independent of the currently loaded spec configuration, exactly as A1 requires for its automatic publication intents. The issue becomes dispatch-eligible only after step 6 completes and the intent is terminal. A2 invokes implementation through nothing but this tracker label/state boundary.

### HTTP and events

The existing factory-run endpoints gain the spec stage. A run's `attempts` include spec attempts with per-turn records (turn kind, timing, usage, model, status, error). The response adds a `spec` object:

```json
{
  "spec": {
    "current_version": 2,
    "pending_approval_version": null,
    "approved_version": 2,
    "versions": [
      {
        "artifact_id": "018f...",
        "version": 2,
        "attempt": 2,
        "review_cycles": 1,
        "unresolved_blocking_findings": 0,
        "published": true,
        "received_at": "2026-07-18T18:00:00Z"
      }
    ],
    "revision_requests_used": 1,
    "decision": "approved",
    "publication": {
      "intent_id": "018f...",
      "status": "applied",
      "completed_steps": ["comment_pending", "pin_version", "route_label", "project_state", "label_cleanup", "comment_final"],
      "error": null
    }
  }
}
```

Artifact and findings content is retrievable through `GET /api/v1/factory-runs/{run_id}/artifacts/{artifact_id}`, which returns the validated artifact JSON plus its metadata (version, attempt, received timestamp) inside the existing envelope, `404` for an unknown ID, and the standard error envelope otherwise. Version entries in the `spec` object carry the artifact ID for this lookup. On runs that carry both triage and spec stages, the existing top-level `artifact` and `publication` fields remain triage-owned and unchanged; spec data appears only under `spec`, and `current_stage` reflects the most recently active stage. Status enums, error envelope, and field bounds follow A1. The metrics endpoint accepts `stage=spec` and returns attempt, failure, review-cycle, convergence (attempts completing with zero unresolved blocking findings), revision-request, approval-latency (publication-to-decision), duration, and token aggregates grouped by harness and model.

Event names: `spec_started`, `spec_turn_completed`, `spec_completed`, `spec_failed`, `spec_ineligible`, `spec_published`, `spec_revision_requested`, `spec_approved`, `spec_route_applied`, `spec_publication_blocked`, `spec_publication_conflict`. Payloads follow the A1 envelope with `run_id`, `stage_run_id`, artifact/intent/version fields when present, `status`, and `error_code`.

## Error handling

- Intake, persistence, locking, and migration failures follow A1 rules unchanged.
- Any turn failure (spawn, timeout, missing or malformed output, schema violation, verdict/findings mismatch, integrity failure, cancellation) fails the attempt with a durable bounded error naming the failing turn. The issue keeps its intake label. Retry creates a new attempt up to `spec.max_attempts`; exhaustion leaves a durable failed stage with remediation and requires a changed issue or configuration revision.
- Publication failures keep intents pending and retry idempotently; missing labels, invalid states, and authorization failures block with exact remediation; human mutation conflicts stop with a conflict record. The spec intake label is never removed by a blocked or incomplete approval publication.
- Restart follows A1: stale leases interrupt attempts, exact process-identity signaling, attempt cleanup, intents resume from expected projections, completed artifacts immutable. An attempt interrupted mid-pipeline restarts from its first turn (draft for cold attempts, seeded revise for revision attempts) as a new attempt; completed turn records of the interrupted attempt remain inspectable.
- A blocked or conflicted approval intent leaves `pending_approval_version` set, `approved_version` unset, and the owned comment showing the block or conflict with remediation. Resolution is human: fix the blocking condition and reconciliation resumes, or change the issue so a new revision pair supersedes the pending decision.
- Exceeding `spec.max_revision_requests` blocks further revision attempts, updates the owned comment with remediation (a maintainer edits the issue or spec expectations and re-labels, producing a new revision pair), and exposes the blocked state over HTTP and events.

## Security and trust boundaries

A1's model applies unchanged: issue content, comments, repository content, and model output are untrusted; the local runner and host account are trusted; the runner receives no forge mutation capability; only schema-validated artifacts reach the publisher; only configured labels and states are applied; credentials stay in the adapter/publisher process; all persisted and displayed strings are bounded.

A2-specific notes:

- Human feedback comments are untrusted input passed to the revise turn as data files, never as executable configuration.
- Decision labels are read from GitHub without author attribution; anyone with triage permission on the repository can approve. This matches the existing trust level of tracker-driven implementation dispatch and is recorded as an accepted limitation until Horizon B2/C1 add identity and policy.
- The reviewer turn's fresh context is an isolation property enforced by the coordinator, not by the model.

## User-visible behavior

### PR1: spec generation preview

Labeling a project-member issue `ready-to-spec` produces one owned comment with the full versioned spec, any unresolved blocking findings, run and attempt IDs, and a preview notice. No labels or states change. Off-project issues receive the diagnostic comment. The factory-run API shows attempts, turns, findings, versions, and publication status. Doctor and startup output identify the spec stage and its mode.

### PR2: approval and revision

Applying `spec-approved` produces the approval flow ending in the implement label and optional `Todo` state, with the owned comment showing `approved — implementation-ready`. Applying `spec-revise` with a feedback comment produces version N+1 in the same comment. Stale approvals, missing feedback, conflicting labels, and exhausted revision budgets each produce a visible diagnostic on the owned comment.

## Delivery slices

### Pull request 1: spec generation preview

Delivers: spec configuration and validation; store extensions for spec attempts, turn records, findings, and versioned artifacts; spec intake; pipeline coordinator with bounded review loop; reviewer model resolution; spec comment publication with preview notice; ineligible diagnostics; HTTP and metrics additions; events; doctor checks; starter prompts; docs; automated tests; preview UAT.

User value: maintainers get reviewed, versioned specs on spec-routed issues and can evaluate quality before enabling routing effects.

### Pull request 2: approval, revision, and implement handoff

Delivers: decision-label detection; approval publication intent with pinning and label/state effects; revision attempts seeded with prior spec and feedback; stale-approval and conflict handling; revision budget enforcement; approval metrics; UAT covering the full PRD A2 demo.

User value: the PRD A2 journey is complete: spec, request one change, approve the revision, and the run becomes implementation-ready.

Merge gates:

| Surface | PR1 | PR2 |
| --- | --- | --- |
| Acceptance criteria | 1–7, 13, 15 in full; PR1 portions of 11–12, 14 | 8–10, 16 in full; remaining portions of 11–12, 14 |
| Config keys | all except decision-label and approval-route enforcement paths | decision-label and approval-route behavior |
| Events | `spec_started`, `spec_turn_completed`, `spec_completed`, `spec_failed`, `spec_ineligible`, `spec_published`, `spec_publication_blocked` | `spec_revision_requested`, `spec_approved`, `spec_route_applied`, `spec_publication_conflict` |
| HTTP | attempts, turns, `spec.versions`, artifact sub-resource, publication; metrics: attempt, failure, review-cycle, convergence, duration, tokens | `pending_approval_version`, `approved_version`, decision, revision counts; metrics: revision-request, approval-latency |
| UAT | criterion 15: spec-labeled issue → published versioned spec; off-project diagnostic; restart during spec-comment publication | criterion 16: revise → version 2; approve → implementation-ready; restart during approval publication |

## Acceptance criteria

1. `WORKFLOW.md` can enable the spec stage with intake label (default `ready-to-spec`), three required prompts, `spec.max_intake_pages` (default 100, > 0), `spec.max_review_cycles` (default 3, > 0), `spec.max_attempts` (default 3, > 0), `spec.max_revision_requests` (default 3, > 0), per-turn timeout, optional `spec.model` and `spec.review_model` with Pi precedence `spec.model` → `agent.model` → harness default and reviewer defaulting to the drafter's resolved model, and decision labels distinct from the spec intake and approval-route labels and, when triage is configured, from the triage intake and route labels. Config validation rejects `spec.model` and `spec.review_model` when `agent.name` is `codex`, and doctor reports a triage-`spec`-route/intake-label mismatch.
2. Intake paginates open spec-labeled project-member issues to exhaustion; reaching `spec.max_intake_pages` in any completeness-sensitive read fails the whole operation visibly with no attempts from partial results; an off-project spec-labeled issue receives one idempotent diagnostic comment, an ineligible run record, and a `spec_ineligible` event, and starts no agent attempt; an issue carrying both `needs-triage` and the spec intake label is skipped with one durable `spec_ineligible` event (error code `intake_label_conflict`) per issue revision and no comment.
3. Before any agent turn, Symphony persists a factory run (reusing the issue's existing run when one exists) and one nonterminal spec attempt under the exclusive store lock, with stage-scoped uniqueness: at most one nonterminal attempt and one successful artifact per `(stage, issue_revision, configuration_revision)`, terminal failed attempts retained up to `spec.max_attempts`. The store extension is additive and the complete existing triage test suite passes unchanged. Every turn output, findings payload, and published spec version is stored durably and immutably with per-turn timing and usage; published versions increment monotonically per run and only on attempt completion.
4. The pipeline executes draft, review, and revise as separate isolated one-turn invocations, each in a fresh turn-unique workspace and stage input directory containing only that turn kind's allowlisted inputs; the reviewer invocation receives the issue context and current spec only, and a planted draft-turn file is proven absent from the reviewer workspace; the loop stops at a `pass` verdict or at `spec.max_review_cycles`; at the cap, coordinator post-processing appends bounded unresolved-finding summaries to `open_decisions` within schema limits, re-validates once, stores the result as the published artifact, and renders all unresolved blocking findings in the comment. Tests prove the invocation sequence, workspace freshness, context isolation, both stop conditions, cap post-processing at the array bound, and the reviewer model override.
5. Spec artifact validation enforces schema version 1, non-empty bounded `product_behavior` and `technical_approach`, 1–50 bounded `acceptance_criteria`, 0–50 bounded `open_decisions`, the 64 KiB file cap, and unknown-field rejection; findings validation enforces the verdict/blocking-findings consistency rule, severity and section enums, and bounded fields. Any validation failure fails the attempt with a durable bounded error naming the failing turn.
6. Every turn enforces A1's repository-integrity contract and environment isolation; child-process tests prove forge credentials, helper environment, and lifecycle hooks are absent and that source mutations fail the turn while ignored build output does not.
7. PR1 publishes exactly one publisher-owned marked spec comment per run containing all four sections, version, run/attempt IDs, unresolved blocking findings when present, review instructions, and a preview notice; it changes no labels or states; and it reconciles idempotently across crash, restart, and create-before-record windows using A1's ownership contract, with spoofed markers ignored.
8. Applying `spec-approved` to a current published version creates a durable approval intent that executes in order: pending comment recording `pending_approval_version`; removal of the spec intake and both decision labels; implement label; optional Projects v2 state; `approved_version` pin; final comment. HTTP and events report approval only after the final step, exposing `pending_approval_version` until then; each step uses expected-projection reconciliation; crash windows between GitHub success and local recording reconcile without duplication; a managed value differing from both expected and desired projections records a human conflict and stops with `approved_version` unset.
9. The implementation scheduler rejects every issue ID with a nonterminal spec approval publication intent, independent of live spec configuration; dispatch eligibility begins only after the intent is terminal, and tests prove no worker dispatch occurs while the implement label and state are applied but the intent is nonterminal.
10. Decision handling follows the ordered decision table: both decision labels → conflict diagnostic and no effect; `spec-revise` with feedback (a non-Symphony comment created or updated after the last spec publication, including an edited pre-publication comment) → exactly one seeded revision attempt bounded by `spec.max_revision_requests`, producing the next published version in the same owned comment with `spec-revise` removed at republication; `spec-revise` without feedback → diagnostic and no attempt; stale `spec-approved` → `spec_publication_conflict` event and comment diagnostic with no publication and no automatic new attempt; a changed revision pair without a decision label re-enters ordinary intake as a cold attempt. Exceeding `spec.max_revision_requests` blocks with visible remediation on the comment, HTTP, and events.
11. The factory-run endpoints expose spec attempts with per-turn kind, status, timing, usage, and model; a `spec` object with versions, review-cycle and unresolved-finding counts, revision requests used, decision, `pending_approval_version`, `approved_version`, and publication state; and `GET /api/v1/factory-runs/{run_id}/artifacts/{artifact_id}` returning validated artifact or findings JSON with metadata, `404` for unknown IDs. On runs with both stages, triage's top-level `artifact` and `publication` fields are unchanged and spec data appears only under `spec`. `GET /api/v1/factory-runs/metrics?stage=spec` returns attempt, failure, review-cycle, convergence, revision-request, approval-latency, duration, and token aggregates grouped by harness and model; stages other than `triage` and `spec` return `400`.
12. All eleven spec event names are emitted live and durably with A1-envelope payloads carrying run, stage-run, artifact, intent, and version identifiers when present; doctor validates prompts, labels, approval route, Projects v2 access, and isolated harness authentication; startup output reports the spec stage state.
13. Pending publication intents, decision detection, and triage reconciliation continue to progress while a multi-invocation spec attempt is in flight; a test proves a pending intent reconciles during a multi-turn attempt.
14. Automated tests cover configuration validation including Codex rejection and label-uniqueness rules, pipeline sequencing and loop bounds, reviewer workspace and context isolation, model resolution, schema and bounds validation for both artifact types, version numbering and artifact immutability, dual-intake-label conflict, comment ownership and crash windows for spec and approval publication, the full decision table including stale approval, missing feedback, conflicting labels, feedback-edit detection, and revision budgets, the scheduler dispatch guard, restart mid-pipeline and mid-publication, HTTP and metrics contracts, and event emission.
15. Manual PR1 UAT with documented fixtures: label a project issue `ready-to-spec`, observe the published versioned spec comment and durable API records, capture the off-project diagnostic, and restart Symphony during a pending spec-comment publication with idempotent completion.
16. Manual PR2 UAT demonstrates the PRD A2 demo: apply `spec-revise` with one feedback comment and observe version 2; apply `spec-approved` and observe the implement label and `Todo` state applied, the intake and decision labels removed, the pinned approved version over the API, and no implementation dispatch before the intent is terminal; restart Symphony during a pending approval publication and observe idempotent completion. Evidence includes issue URLs, API responses, and logs, with fixtures cleaned up afterward.

## Measures

From the PRD A2 slice, computed from durable records:

- approval cycles per spec (revision requests used before approval);
- time awaiting human input (publication to decision latency);
- review-loop convergence rate (attempts completing with zero unresolved blocking findings) and cycles used;
- token usage per approved spec by harness and model;
- implementation rework attributed to spec gaps (deferred until A3 exists; record the linkage fields now: pinned approved version on the run).

## Testing strategy

### Unit tests

- Spec configuration defaults, validation, Codex rejection, label uniqueness, and triage-route consistency.
- Spec and findings schema validation including verdict consistency and every bound.
- Review-loop state machine: pass, revise, cap, and cycle accounting.
- Fingerprint extensions: managed-label and owned-comment exclusion, feedback-comment inclusion.
- Version numbering and artifact immutability.
- Comment rendering for preview, versions, unresolved findings, diagnostics, and approval states.

### Integration tests

- Mock GitHub intake for spec-labeled issues, off-project diagnostics, and dual-label conflicts.
- Full pipeline with fake runners: draft-only pass, one and multiple revise cycles, cap with unresolved findings, and per-turn failures at each position.
- Reviewer model resolution and context-isolation assertions via fake-runner input capture.
- Spec comment publication recovery across crash and create-before-record windows.
- Decision detection: approval flow with every publication step's crash window, revision flow, stale approval, missing feedback, both-labels conflict, revision budget exhaustion, and human label conflicts against expected projections.
- Implementation scheduler dispatch-guard rejection while a spec approval intent is nonterminal, including after spec configuration reload.
- Pending-intent progress while a multi-turn attempt is in flight (non-starvation).
- Restart with an active mid-pipeline attempt and with a pending approval intent.
- Existing triage integration suite unchanged after the store extension.
- Exact HTTP responses, metrics aggregates, and event envelopes.

### Manual UAT

Per acceptance criteria 15 (PR1) and 16 (PR2), using the repository's established UAT harness and cleanup discipline.

### Quality gate

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

Plus the monorepo affected-package validation required by CI.

## Likely file map

New focused modules expected under `apps/symphony/src/` (Build confirms names; a shared `factory/` or per-stage `spec/` layout both acceptable, keeping persistence, coordination, artifact validation, and publication as separate interfaces):

- spec domain types, artifact and findings validation;
- spec coordinator and pipeline state machine;
- spec comment rendering and publication intents;
- decision detection and approval publication;
- store migrations for spec attempts, turn records, findings, versions, pinning, and revision budgets.

Existing areas likely affected:

- `apps/symphony/src/triage/store.rs` and `migrations/` (generalize stage records or add spec tables)
- `apps/symphony/src/triage/intake.rs`, `runner.rs`, `publisher.rs`, `fingerprint.rs` (reuse seams)
- `apps/symphony/src/config.rs`, `domain.rs`, `doctor.rs`, `starter_assets.rs`
- `apps/symphony/src/http_server.rs`, `event_stream.rs`, `orchestrator.rs`
- `apps/symphony/prompts/`, `apps/symphony/docs/WORKFLOW-REFERENCE.md`, `apps/symphony/tests/`

A refactor lifting shared triage infrastructure (store, runner profile, publisher, fingerprint) into stage-neutral modules is in scope where it serves A2; renaming for its own sake is not.

## Risks and mitigations

### Review loop burns tokens without converging

Bound cycles with `spec.max_review_cycles`, publish at the cap with findings visible, and record convergence metrics so operators can tune prompts and models.

### Reviewer rubber-stamps or nitpicks

Fresh-context invocation, optional cross-model review, and the verdict/blocking-finding consistency rule keep the signal structured. The human gate remains the quality backstop; A2 makes reviewer behavior measurable rather than claiming it is correct.

### Spec attempts are long and collide with lease timeouts

Per-turn lease renewal continues during each invocation, as in A1. The multi-invocation attempt renews between turns; the stale threshold is unchanged.

### Human decisions race publication or content changes

Stale-approval detection keys decisions to the published `(issue_revision, configuration_revision)`; expected-projection reconciliation stops on conflicting human mutations; feedback comments intentionally create new revisions.

### Anyone with label permission can approve

Documented as an accepted limitation consistent with existing tracker-driven dispatch. Identity, roles, and risk policy arrive in Horizons B2 and C1.

### Store generalization destabilizes shipped triage

Extend the store behind its existing interface with additive migrations; triage tests must pass unchanged; shared refactors are limited to seams A2 actually consumes.

### PR1 is large for one review

PR1 spans the store extension, pipeline, intake, publication, and API. The store extension is bounded by the schema listed in this spec and gated by the unchanged triage suite. If Build finds the extension exceeding that boundary, stop under the blocking conditions rather than splitting into a foundation-only PR.

### Dual-stage label vocabulary confuses maintainers

Doctor enforces label uniqueness and triage-route consistency; starter configuration and docs present one coherent vocabulary table.

## Explicitly deferred work

- Linear spec stage and vocabulary mapping.
- Spec artifacts as repository files or spec PRs.
- HTTP, chat, or Pi approval surfaces.
- Multi-turn continuation within one runner invocation.
- Risk-class-conditional pipeline shaping and confidence thresholds.
- Approver identity, roles, and audit (Horizon B2/C1).
- A3 consumption of the pinned approved artifact version.
- Implementation-rework-from-spec-gap measurement (requires A3).
- Cost-in-currency attribution.

## Build handoff

### Approved scope

Build A2 as two user-facing vertical pull requests: spec generation preview, then approval/revision with implement handoff. Preserve all sixteen acceptance criteria across the stack.

### Non-negotiable constraints

- Label-based intake composing with, but not depending on, A1 PR2.
- Durable records before agent execution; every turn output stored immutably.
- Draft → review → revise as separate isolated one-turn invocations; reviewer context isolation; bounded loop publishing at the cap with findings visible.
- One structured spec artifact per version; schema validation before any publication.
- Publisher-owned comments and expected-projection intents for all GitHub effects; intake label removed only by completed approval publication.
- Tracker label/state remains the implementation handoff boundary; A2 dispatches nothing directly.
- Every pull request has a tracker-visible demo and durable evidence.

### Build sequence

1. Trace the A1 store, runner, publisher, intake, and fingerprint seams; implement the store schema extension defined in this spec with the triage suite green before building spec features on it.
2. Implement PR1 end to end: config, store, intake, pipeline, comment, API, events, doctor, prompts, docs, tests, preview UAT.
3. Review preview evidence (convergence, findings quality, cost) before building PR2.
4. Implement PR2 end to end: decision detection, approval and revision flows, conflict handling, metrics, UAT.
5. Run focused tests per slice and the full quality gate before completion.
6. Update the OKF roadmap, logs, PRD progress table, and Symphony reference docs after each merged slice; add an ADR for durable spec-stage decisions.

### Verification contract

Build completion requires all sixteen acceptance criteria passing or explicitly blocked with evidence; focused unit and integration suites; the full Rust quality gate and affected-package CI validation; real GitHub UAT evidence for the complete PRD A2 demo including restart; an adversarial code review with no unresolved blockers; and a completion report listing commits, commands, evidence paths, and residual risks.

### Blocking conditions

Stop Build and request a decision if:

- the A1 store or runner cannot support a second stage or multi-invocation attempts without a redesign larger than the traced seams suggest;
- reviewer context isolation cannot be enforced for a supported harness;
- Codex cannot run the pipeline under harness defaults while config validation rejects model overrides;
- decision-label detection cannot distinguish stale approvals reliably from GitHub data;
- a user-facing slice would require a foundation-only pull request;
- an acceptance criterion requires expanding into A3 or Horizon B work.
