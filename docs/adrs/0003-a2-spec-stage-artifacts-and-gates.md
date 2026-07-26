---
type: ADR
title: A2 spec-stage artifacts and human gates
status: Accepted
description: Stage-scoped attempts, isolated review turns, immutable versioned specs, tracker decisions, and pinned implementation handoff for A2.
tags: [symphony, spec-stage, adr, sqlite, github]
timestamp: 2026-07-26T00:00:00Z
---

# ADR-0003: A2 spec-stage artifacts and human gates

## Status

Accepted with the A2 implementation.

## Context

A2 turns a spec-routed GitHub issue into reviewed product and technical behavior that a human can revise or approve. It must coexist with A1 on one factory run, preserve each model turn across restarts, prevent reviewer context leakage, and hand implementation an exact approved artifact. See [A2 Spec Stage](/specs/2026-07-18-a2-spec-stage-design.md).

## Decision

1. **Stage-scoped attempts** — Nonterminal uniqueness includes `stage`, allowing triage and spec attempts for the same issue/configuration revision without collision.
2. **One immutable artifact per published version** — Draft and revise turn outputs remain turn records. A completed attempt stores one schema-validated spec artifact with a monotonically increasing run-local version.
3. **Fresh invocation per turn** — Draft, review, and revise use fresh clone, home, input, and output directories. Review receives only issue context and the current spec; no prior conversation or findings are available.
4. **Bounded adversarial loop** — Review cycles are bounded. At the cap, deterministic coordinator post-processing places bounded unresolved findings in open decisions and preserves the complete blocking-finding set as artifact metadata.
5. **Tracker-owned human gate** — `spec-approved` and `spec-revise` are the decision surface. Feedback is issue content, and edited pre-publication comments count when their update timestamp follows publication.
6. **Pinned implementation handoff** — Approval records the pending version, applies configured tracker effects, pins the exact artifact ID/version, and only then reports the run approved. Nonterminal approval intents block implementation dispatch.
7. **Shared factory store, typed A2 tables** — A2 reuses A1's SQLite lock, run, stage, event, recovery, runner, and comment ownership boundaries while adding spec turns, artifacts, run state, and publication intents additively.

## Consequences

- A factory run can expose both triage's existing top-level artifact and a versioned spec history.
- Review isolation costs one fresh harness invocation and clone per turn but prevents hidden draft context from weakening adversarial review.
- Humans can revise and approve entirely from GitHub while A3 receives a stable approved artifact reference.
- Linear and repository-backed spec PRs remain separate future slices.

## Links

- Spec: [A2 Spec Stage](/specs/2026-07-18-a2-spec-stage-design.md)
- Verify: [A2 UAT Verify Report](/specs/2026-07-26-a2-uat-verify-report.md)
- PRD: [Symphony Software Factory Platform](/specs/symphony-software-factory-platform-prd.md)
- Foundation: [ADR-0001](/adrs/0001-a1-triage-durability-and-isolation.md)
- Implementation: `apps/symphony/src/spec/`, `apps/symphony/src/triage/migrations/003_spec_stage.sql`
