# Specs Update Log

## 2026-07-25
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
