# Specs Update Log

## 2026-08-01
* **Typed factory state is now visible in the TUI**: Specification, implementation, and review lifecycle events feed a live factory-session snapshot, including active rows, bounded completions, stage usage totals, and issue identifiers. The TUI no longer depends on legacy worker state for typed-stage progress; the additive snapshot field remains backward-compatible for API consumers.
* **Automatic publication and review hardening**: Automatic implementation refreshes the configured remote base before capturing the attempt SHA, verifies the completion route after label/state mutations, and branch publication verifies that the pinned base remains reachable before importing the thin result bundle. The review coordinator resolves relative repository/workspace paths before isolated worker setup. Missing base history is a terminal publication conflict with no branch push or retry loop.
* **Implementation preview runtime hardening**: Relative workflow workspace paths now resolve to absolute paths before child-process execution and Git bundle creation. Validation recognizes shell command substitution, preserving the existing UAT command form. Direct UAT on `gannonh/uat-symphony` Project #16 verified preview publication, bundle metadata, and tracker preservation.

## 2026-07-30
* **Operator recovery for blocked publication intents**: [#609](https://github.com/gannonh/kata-symphony/pull/609) adds `symphony publication list-blocked` / `reset <intent-id>`, returning a `blocked` intent to `pending` with `retry_count` cleared and completed steps preserved, recorded as an `implementation_publication_reset` event. Both commands call the orchestrator's admin HTTP routes first and fall back to the durable store only when nothing answers, because the store's exclusive lock is held for as long as Symphony runs. The [A4 spec](/specs/2026-07-30-a4-agent-review-stage-design.md) drops this from its risk list; A4 extends the same command surface and HTTP-first shape to review intents.
* **A4 spec added**: [A4 Agent Review Stage design](/specs/2026-07-30-a4-agent-review-stage-design.md) promoted to Planned — draft PR to structured, read-only agent review with schema-validated findings and a deterministic comment publisher.
* **A3 PR2 shipped; live UAT deferred**: [#607](https://github.com/gannonh/kata-symphony/pull/607) merged as `d456c051`. Final remediation bounded automatic publication reconcile retries with exponential backoff and a `blocked` ceiling, and kept issue-revision drift off that budget as a non-budgeted waiting state so a slow human re-approval cannot strand publication. Live AC20 UAT was **not executed** and is deferred by maintainer decision to keep the roadmap moving; the [verify report](/specs/2026-07-29-a3-pr2-verify-report.md) remains Incomplete and is not superseded. A4 proceeds without it.

## 2026-07-29
* **A3 PR2 review remediation**: [#607](https://github.com/gannonh/kata-symphony/pull/607) now pins the forge repository/branch, authenticates bounded and redacted Git subprocesses, revalidates persisted draft PRs before routing, preserves retryable recovery, and validates publication configuration in doctor. All original inline findings are addressed; automated gates Pass and live AC20 UAT remains documented as residual.
* **A3 PR2 implemented**: Deterministic draft-PR publication and Agent Review handoff ([build](/specs/2026-07-29-a3-pr2-build-report.md), [verify](/specs/2026-07-29-a3-pr2-verify-report.md) Incomplete). Automated gates Pass; live UAT residual. Next: live PR2 UAT, then A4.
* **A3 next = PR2**: Specs roadmap / PRD slice table now show PR1 as Implemented ([#606](https://github.com/gannonh/kata-symphony/pull/606)) and PR2 as Next.
* **A3 PR1 review remediation (#606)**: Validation credential isolation + timeout output tails; discrete `$SYMPHONY_STAGE_INPUT` files; approved-spec verified via `git show HEAD:`; claim-time `pending` run state + eligibility exclude non-`failed` decisions; spawn identity recording; Docker fail-closed; bundle/temp/store hardening.
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
