# ADRs Update Log

## 2026-08-06
* **Updated**: [ADR-0006](0006-a5-verification-evidence-and-gate.md) records credential environment scrubbing (startup capture, post-config scrub, explicit forwarding to agent sessions, `core.filemode=true` pinned-tree attestation) and the end-state attestation boundary for transient mutations (read-only/overlay isolation is a Linux-only follow-up).

## 2026-08-02
* **Accepted**: [ADR-0005 A4 durable review publication fencing](0005-a4-review-publication-fencing.md) records active lease CAS writes, heartbeat renewal during forge calls, changed-head supersession ownership, and the explicit operator reset boundary.

## 2026-08-02
* **Updated**: [ADR-0004](/adrs/0004-a3-implementation-durability-and-bundles.md) records the additive typed factory snapshot used by the TUI and the completed direct draft-PR, Agent Review preview, and Ratatui UAT evidence on Project #16. Restart-during-publication, cleanup, and Docker coverage remain residuals.

## 2026-07-30
* **Updated**: [ADR-0004](/adrs/0004-a3-implementation-durability-and-bundles.md) records that publication recovery is served over the admin HTTP surface with a direct-store fallback. A store-only command could never work while Symphony ran, because the orchestrator holds the store's exclusive lock for its lifetime — the exact moment a blocked intent is discovered.
* **Updated**: [ADR-0004](/adrs/0004-a3-implementation-durability-and-bundles.md) now documents the operator recovery path for a `blocked` publication intent (`symphony publication list-blocked` / `reset`), closing the gap where the ADR promised operator recovery that no code path provided. `conflict` remains deliberately terminal.

## 2026-07-29
* **Updated**: [ADR-0004](/adrs/0004-a3-implementation-durability-and-bundles.md) bounds automatic publication reconcile attempts with exponential backoff and a retry ceiling that terminalizes an exhausted intent as `blocked` instead of retrying forever on every poll; the ceiling counts failed attempts only, so issue-revision drift awaiting human re-approval cannot exhaust it and strand publication.
* **Updated**: [ADR-0004](/adrs/0004-a3-implementation-durability-and-bundles.md) now pins automatic publication identity, scopes Git token use to bounded subprocesses, and requires live PR revalidation before tracker handoff.
* **Updated**: [ADR-0004](/adrs/0004-a3-implementation-durability-and-bundles.md) extended for PR2 progressive publication, draft-PR artifacts (`005_implementation_draft_pr.sql`), and no-force expected-projection branch/PR rules.
* **Accepted**: [ADR-0004 A3 implementation durability and bundles](/adrs/0004-a3-implementation-durability-and-bundles.md) records stage-scoped A3 tables, content-addressed Git bundles beside SQLite, credential-isolated local execution, and preview-only publication before PR2 draft-PR handoff.

## 2026-07-26
* **Accepted and linked**: [ADR-0003 A2 spec-stage artifacts and human gates](/adrs/0003-a2-spec-stage-artifacts-and-gates.md) records stage-scoped attempts, fresh-context review turns, immutable spec versions, tracker decisions, and approved-artifact pinning; linked from the accepted [A2 UAT verify report](/specs/archive/2026-07-26-a2-uat-verify-report.md).

## 2026-07-25
* **Accepted and updated**: [ADR-0002 triage process recovery identity](/adrs/0002-triage-process-recovery-identity.md) authorizes recovery by PID, process group, and OS start token while treating executable identity as diagnostic; review remediation adds unresolved-state retention, Pi/Codex process-group isolation, cleanup-root authorization, and the remaining spawn-to-persistence window.

## 2026-07-18
* **Accepted**: [ADR-0001 A1 triage durability and isolation](/adrs/0001-a1-triage-durability-and-isolation.md) records SQLite durability, forge-host mapping, membership keys, runner isolation, lease renewal, and preview intent recovery from PR1.

## 2026-07-15
* **Initialization**: Seeded empty ADRs section for future architecture decisions.
