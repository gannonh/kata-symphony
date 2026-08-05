---
type: ADR
title: A5 exact-head verification evidence and the deterministic gate
status: Accepted
description: Head/base review identity, exact-head bundle execution with pre-release launch barriers, digest-addressed evidence, a verifier-proof deterministic gate, and failed-gate hold semantics.
tags: [symphony, software-factory, a5, verification, evidence, gate]
timestamp: 2026-08-05T12:00:00Z
---

# ADR-0006: Exact-head verification evidence and the deterministic gate

- Status: Accepted
- Date: 2026-08-05
- Constrains: [A5.1 reviewed PR verification evidence preview](https://github.com/gannonh/kata-symphony/issues/617) and its parent epic [A5 reviewed PR verification](https://github.com/gannonh/kata-symphony/issues/616)

## Context

A3 runs repository validation before draft-PR publication. A4 runs after A3
against the exact reviewed PR head so post-review changes cannot bypass final
verification. A5 runs after A4 against the exact reviewed PR head and must
produce trustworthy, durable evidence that an acceptance path can be
evaluated against. Earlier stages established durable artifacts, credential
isolation, verified Git bundles, and content-addressed blob storage, but no
stage executes arbitrary repository-owned commands against a pinned PR head or
attests acceptance criteria from that evidence.

## Decision

1. **A5 is a typed `verification` stage** backed by `stage_runs`, with the
   A4 review-cycle identity extended to `(run_id, reviewed_head_sha,
   base_sha)`. A change to either SHA requires a fresh A4 artifact and
   publication (migration 009 rebuilds the review tables preserving every
   existing artifact, finding, and publication reference).

2. **Eligibility requires an applied automatic A4 publication.** Preview
   eligibility is `durable A4 findings artifact` + `applied automatic A4
   publication` whose deterministic route equals `verification.trigger_state`
   + `tracker item in that state`. A4 preview publication, tracker state
   alone, an incomplete automatic publication, or a non-completion route never
   starts A5. `review.completion_route.state` must equal
   `verification.trigger_state` and `changes_requested_route.state` must
   differ, enforced at config validation.

3. **Exact-head execution.** The controller fetches `refs/pull/<number>/head`
   with subprocess-scoped authentication (`GIT_CONFIG_*` header, never argv),
   verifies the fetched SHA equals the A4 reviewed head, and creates a
   credential-free bundle clone for a disposable local or Docker workspace.
   HEAD, the committed tree, and tracked files are re-verified after command
   execution. The live head/base are re-read before workspace creation, after
   commands, after the verifier, and before publication; any change supersedes
   the attempt without publishing.

4. **Pre-release launch barriers.** Local commands run through a
   new-process-group supervisor blocked on a controller-owned pipe. A
   `launching` record and nonce are persisted before spawn; the supervisor's
   PID/process-group/OS start token/executable identity is durably
   CAS-recorded before the payload runs; pipe closure before release makes the
   supervisor exit without running the payload. Docker commands use labeled
   `docker create`, persist the container ID before `docker start`, and
   recovery removes label-discoverable stopped orphans. Timeout, restart, or
   cancellation terminates and reaps the persisted process group or container,
   verifies termination, and records an interrupted result before cleanup.

5. **Evidence is digest-addressed and metadata-only on the wire.** Evidence
   collection accepts only bounded regular files below the attempt-owned
   `$SYMPHONY_EVIDENCE_DIR`; traversal errors, symlinks, special files,
   file-count overflow, aggregate-size overflow, and post-hash mutation fail
   closed. The shared atomic blob helper hashes the staged copy after source
   copy and compares staged digest/size with the intended identity before
   rename. Physical blob paths and binary bytes are absent from the
   unauthenticated HTTP API.

6. **Symphony owns the gate.** A read-only verifier receives only the pinned
   A2 spec, A3 implementation claims, A4 findings, recorded command results,
   and stored evidence metadata, and emits a strict criterion matrix. The
   strict manifest rejects unknown fields, wrong schema/head/base/spec
   identities, duplicate or missing criteria, unsupported statuses, empty
   rationale, and references to evidence outside the attempt. The gate passes
   only when every command completed and passed (including the single
   acceptance command) AND every approved acceptance criterion appears exactly
   once as `pass` with valid stored evidence references. A `fail` or
   `not_proven` criterion holds the gate: the verifier cannot waive it, and
   the failed gate stays in Verification without auto-retry. Eligibility
   guards exclude completed attempts for the reviewed head/base pair and cap
   failed/interrupted attempts at `max_attempts`, so repeated crashes cannot
   reclaim the same revision pair.

7. **Failed gates are product evidence and hold.** A completed failed gate
   remains in `Verification`, is never auto-retried, and does not consume the
   retry budget. Disabling verification stops new claims and automatic
   retries; cleanup and reconciliation continue until owned processes,
   containers, workspaces, and pending publications are safely terminal or
   recoverable.

## Consequences

- A5 preview mode performs exactly one owned issue comment and no other
  tracker or PR mutation; tracker routing, same-head retry controls, PR
  checks, remediation, approval, merge, and deployment remain out of scope
  (A6).
- Local commands are trusted workflow configuration; the local profile
  guarantees cleanup of the persisted process group, not OS confinement
  against an adversarial command. Docker provides the stronger containment
  profile.
- Acceptance commands must be repeatable after an interrupted attempt, and
  retries only ever run from a fresh credential-free workspace.
- Cross-linked: [ADR-0004](../adrs/0004-a3-implementation-durability-and-bundles.md),
  [ADR-0005](../adrs/0005-a4-review-publication-fencing.md), parent epic
  [#616](https://github.com/gannonh/kata-symphony/issues/616).
