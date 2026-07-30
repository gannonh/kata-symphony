# ADRs Update Log

## 2026-07-30
* **Updated**: [ADR-0004](/adrs/0004-a3-implementation-durability-and-bundles.md) now documents the operator recovery path for a `blocked` publication intent (`symphony publication list-blocked` / `reset`), closing the gap where the ADR promised operator recovery that no code path provided. `conflict` remains deliberately terminal.

## 2026-07-29
* **Updated**: [ADR-0004](/adrs/0004-a3-implementation-durability-and-bundles.md) bounds automatic publication reconcile attempts with exponential backoff and a retry ceiling that terminalizes an exhausted intent as `blocked` instead of retrying forever on every poll; the ceiling counts failed attempts only, so issue-revision drift awaiting human re-approval cannot exhaust it and strand publication.
* **Updated**: [ADR-0004](/adrs/0004-a3-implementation-durability-and-bundles.md) now pins automatic publication identity, scopes Git token use to bounded subprocesses, and requires live PR revalidation before tracker handoff.
* **Updated**: [ADR-0004](/adrs/0004-a3-implementation-durability-and-bundles.md) extended for PR2 progressive publication, draft-PR artifacts (`005_implementation_draft_pr.sql`), and no-force expected-projection branch/PR rules.
* **Accepted**: [ADR-0004 A3 implementation durability and bundles](/adrs/0004-a3-implementation-durability-and-bundles.md) records stage-scoped A3 tables, content-addressed Git bundles beside SQLite, credential-isolated local execution, and preview-only publication before PR2 draft-PR handoff.

## 2026-07-26
* **Accepted and linked**: [ADR-0003 A2 spec-stage artifacts and human gates](/adrs/0003-a2-spec-stage-artifacts-and-gates.md) records stage-scoped attempts, fresh-context review turns, immutable spec versions, tracker decisions, and approved-artifact pinning; linked from the accepted [A2 UAT verify report](/specs/2026-07-26-a2-uat-verify-report.md).

## 2026-07-25
* **Accepted and updated**: [ADR-0002 triage process recovery identity](/adrs/0002-triage-process-recovery-identity.md) authorizes recovery by PID, process group, and OS start token while treating executable identity as diagnostic; review remediation adds unresolved-state retention, Pi/Codex process-group isolation, cleanup-root authorization, and the remaining spawn-to-persistence window.

## 2026-07-18
* **Accepted**: [ADR-0001 A1 triage durability and isolation](/adrs/0001-a1-triage-durability-and-isolation.md) records SQLite durability, forge-host mapping, membership keys, runner isolation, lease renewal, and preview intent recovery from PR1.

## 2026-07-15
* **Initialization**: Seeded empty ADRs section for future architecture decisions.
