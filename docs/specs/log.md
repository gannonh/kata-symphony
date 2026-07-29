# Specs Update Log

## 2026-07-29
* **A3 PR1 implemented**: Preview path (eligibility, local runner, validation/repair, bundles, preview publisher, HTTP/metrics) landed with [build](/specs/2026-07-29-a3-pr1-build-report.md) and [verify](/specs/2026-07-29-a3-pr1-verify-report.md) reports plus [ADR-0004](/adrs/0004-a3-implementation-durability-and-bundles.md). Automated gates Pass; live GitHub/Docker UAT residual. PR2 draft-PR publication is next on the roadmap.
* **A3 PR1 foundation**: Store migration `004_implementation_stage.sql`, eligibility/guards, config validation, doctor checks, HTTP/metrics stubs, and starter prompts/reference comments landed under `apps/symphony/src/implementation/`. Full coordinator/runner/preview loop still open within PR1.

## 2026-07-27
* **A1 PR3 shipped; A1 GitHub path complete**: Marked A1 PR3 recovery and agreement measurement shipped in [#599](https://github.com/gannonh/kata-symphony/pull/599) (`c52d23dc`). [A1 design](/specs/2026-07-16-a1-github-issue-triage-design.md) status is Completed (Linear deferred); roadmap Blocked cleared; [re-verify](/specs/2026-07-25-a1-pr3-reverify-report.md) Accepted on maintainer closeout. A3 remains the next Active factory slice.
* **A1 PR2 shipped; A3 next**: Marked A1 PR2 automatic route publication shipped in [#598](https://github.com/gannonh/kata-symphony/pull/598) (`6a454fe9`). Promoted [A3 Implementation Stage](/specs/2026-07-26-a3-implementation-stage-design.md) from Draft/Planned to Active as the next factory slice; updated [specs roadmap](/specs/index.md), [A1 design](/specs/2026-07-16-a1-github-issue-triage-design.md), [A2 design](/specs/2026-07-18-a2-spec-stage-design.md), and [factory PRD](/specs/symphony-software-factory-platform-prd.md).

## 2026-07-26
* **A3 draft**: Added [A3 Implementation Stage](/specs/2026-07-26-a3-implementation-stage-design.md), defining A2-approved-only intake, credential-isolated local and Docker implementation, committed approved-spec records, deterministic validation and repair, durable Git bundles, Symphony-owned draft-PR publication, and Agent Review handoff. A3 is Planned on the shipped, live-verified A2 pinning boundary.
* **A2 UAT accepted**: [Verify report](/specs/2026-07-26-a2-uat-verify-report.md) signs off criteria 15–16 plus criterion 11 metrics/token measures on `gannonh/uat-symphony` Project #16. [A2 Spec Stage](/specs/2026-07-18-a2-spec-stage-design.md) status is Completed; roadmap and PRD progress updated.
* **A2 implementation**: [A2 Spec Stage](/specs/2026-07-18-a2-spec-stage-design.md) GitHub tracker workflow implemented end to end: durable isolated review pipeline, versioned publication, human revision/approval, approved artifact pin, and implementation route handoff.

## 2026-07-25
* **A1 PR3 review remediation**: [PR #599](https://github.com/gannonh/kata-symphony/pull/599) has all review threads resolved and required CI checks passing after Codex process isolation, recovery-state retention, cleanup authorization, and latest-per-run randomized correction candidate fixes. Updated the [build report](/specs/2026-07-25-a1-pr3-build-report.md), [re-verify report](/specs/2026-07-25-a1-pr3-reverify-report.md), A1 design, PRD, and roadmap. Live acceptance remains blocked.
* **A1 PR3 Verify rejected**: Live GitHub UAT passed retry records, preview/automatic publication, correction events, metrics, and dedupe, but restart recovery left a post-`exec` orphan alive because the executable identity changed from the recorded launcher. See [Verify report](/specs/2026-07-25-a1-pr3-verify-report.md).
* **A1 PR3 build**: Interrupted-attempt recovery and human route correction measurement implemented in Symphony triage; see [A1 PR3 build report](/specs/2026-07-25-a1-pr3-build-report.md). Adds `triage::process_identity` (identity-matched bounded termination, attempt cleanup) and `triage::correction` (agreement/correction/ambiguity comparison, `triage_route_corrected`, durable-only `triage_route_consistency`). Automated gates pass; live restart/correction UAT on `gannonh/uat-symphony` pending Verify.

## 2026-07-24
* **A1 PR2 Verify accepted**: [Verify report](/specs/2026-07-24-a1-pr2-verify-report.md) signed off after live UAT on Project #16; PR2 merge still pending. PR3 remains planned.
* **A1 PR2 build**: Automatic route publication + implement handoff implemented in Symphony triage/orchestrator; see [A1 PR2 build report](/specs/2026-07-24-a1-pr2-build-report.md).

## 2026-07-18
* **A1 PR1 shipped**: Marked [A1 GitHub Issue Triage](/specs/2026-07-16-a1-github-issue-triage-design.md) Active with PR1 preview complete ([#587](https://github.com/gannonh/kata-symphony/pull/587)); roadmap lists PR2/PR3 as planned and PR1 under completed.
* **PRD**: Updated [Symphony Software Factory Platform PRD](/specs/symphony-software-factory-platform-prd.md) to Active; A1 progress table and narrowed platform gaps for durable GitHub triage preview.

## 2026-07-17
* **A1 Preview**: Symphony A1 triage preview implementation is active — HTTP factory-run read routes, doctor triage checks, and starter `prompts/triage.md` / workflow reference updates. Design remains Draft.

## 2026-07-16
* **A1 Draft**: Added [A1 GitHub Issue Triage](/specs/2026-07-16-a1-github-issue-triage-design.md), defining Projects v2 intake, durable triage runs, preview and automatic routing, restart reconciliation, and acceptance evidence.
* **PRD**: Added [Symphony Software Factory Platform](/specs/symphony-software-factory-platform-prd.md), defining the end-to-end product model, delivery principles, success measures, and vertical-slice roadmap.

## 2026-07-15
* **Initialization**: Created specs section; [index.md](/specs/index.md) is the OKF roadmap linking into existing Superpowers specs and plans under [`/superpowers/`](/superpowers/).
