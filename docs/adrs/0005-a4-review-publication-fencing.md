---
type: ADR
title: A4 durable review publication fencing
status: Accepted
description: Active leases and head-bound publication identities fence concurrent formal review publication, routing, recovery, and supersession writes.
tags: [symphony, software-factory, a4, review, durability]
timestamp: 2026-08-02T21:10:00Z
---

# ADR-0005 A4 durable review publication fencing

## Status

Accepted for A4 PR2 on `feat/a4-review-publication`.

## Context

Formal review publication spans GitHub review creation, durable identity and finding records, Projects v2 routing, and finalization. A restart or concurrent coordinator can observe the same pending intent while an earlier publisher is still completing a forge operation. Changed PR heads also invalidate an in-progress review cycle.

## Decision

1. Every worker-owned publication-row mutation uses a compare-and-set requiring `status = 'pending'`, the caller's lease owner, and an unexpired lease. Terminal rows reject stale step, identity, route, completion, error, and supersession writes.
2. Formal, preview, and Projects v2 operations claim the intent before forge I/O. A one-second heartbeat renews the 900-second lease during each operation. Durable predicates remain authoritative when renewal fails or ownership changes.
3. Changed-head supersession claims the same lease before terminalizing the stale intent as `conflict`. It preserves the retry budget and clears the lease only in the successful terminal update.
4. Review identity recovery validates the authenticated publisher, marker ownership, and live PR head before recording `review_created`. Missing durable PR identity fails closed.
5. The blocked/conflict reset endpoint remains an explicit operator recovery command. It uses a status CAS, clears the prior lease, preserves completed steps and forge identities, and records the operator event.
6. Mainline and maintained UAT workflows stay preview-only. Automatic publication is exercised only in isolated formal UAT workflows.

## Consequences

- Concurrent coordinators cannot overwrite a pending intent owned by another active publisher or resurrect a terminal intent.
- A healthy slow forge request renews its lease while the request is in flight. An external request already accepted by GitHub or Projects v2 cannot be cancelled by SQLite; marker ownership, live-head checks, reconciliation, and idempotent route writes provide recovery after process failure or provider delay.
- Explicit operator reset is auditable and separate from worker publication ownership. HTTP control authentication remains a later platform slice.

## Verification

- Serial `cargo fmt --check`, Clippy with `-D warnings`, and the full Symphony suite pass.
- Store tests cover active and expired leases, terminal stale writers, retry preservation, and supersession ownership.
- Formal UAT against `gannonh/uat-symphony` PR #46 produced one `COMMENTED` review, applied all four durable steps, routed to `Human Review`, and restored the Project item to `Agent Review`. Evidence is under `/tmp/kata-symphony-current-uat-evidence/a4-pr2/formal-latest/`.

## Links

- [A4 Agent Review Stage](../specs/archive/2026-07-30-a4-agent-review-stage-design.md)
- [A4 PR2 verify report](../specs/archive/2026-08-02-a4-pr2-verify-report.md)
