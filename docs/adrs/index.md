# Architecture Decision Records

Durable architecture decisions (OKF concept docs with `type: ADR`).

When `/domain-modeling` or architecture work records a decision, place it under this directory and list it below.

# Accepted

* [ADR-0001 A1 triage durability and isolation](0001-a1-triage-durability-and-isolation.md) - SQLite factory runs, forge-host identity, Projects v2 membership keys, local runner isolation, lease renewal, preview intents
* [ADR-0002 Triage process recovery identity](0002-triage-process-recovery-identity.md) - PID, process group, and OS start token authorize recovery; executable identity is diagnostic
* [ADR-0003 A2 spec-stage artifacts and human gates](0003-a2-spec-stage-artifacts-and-gates.md) - stage-scoped attempts, isolated review turns, immutable versions, tracker decisions, pinned implementation handoff
* [ADR-0004 A3 implementation durability and bundles](0004-a3-implementation-durability-and-bundles.md) - stage-scoped A3 records, content-addressed Git bundles, credential-isolated local execution, preview-only publication
* [ADR-0005 A4 durable review publication fencing](0005-a4-review-publication-fencing.md) - active lease CAS fencing, heartbeat renewal, changed-head supersession, and explicit operator recovery
* [ADR-0006 A5 verification evidence and gate](0006-a5-verification-evidence-and-gate.md) - head/base review identity, exact-head bundle execution, pre-release launch barriers, digest-addressed evidence, verifier-proof deterministic gate, failed-gate hold

# Proposed

_None yet._

# Superseded

_None yet._
