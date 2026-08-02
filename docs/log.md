# Docs Update Log

## 2026-08-01
* **Typed factory state is now visible in the TUI**: Specification, implementation, and review lifecycle events feed a live factory-session snapshot, including active rows, bounded completions, stage usage totals, and issue identifiers. The TUI no longer depends on legacy worker state for typed-stage progress; the additive snapshot field remains backward-compatible for API consumers.
* **Automatic publication and review hardening**: Automatic implementation refreshes the pinned remote base before capturing its commit, verifies the completion route after label/state mutations, and publication rejects a captured base that is absent from the remote before importing a thin result bundle. The review coordinator resolves relative repository/workspace paths before isolated worker setup. These checks prevent local-only UAT/configuration commits, Projects automation races, and relative-path failures from producing opaque UAT stalls.
* **Implementation preview UAT completed**: Fixed relative Git bundle destinations, canonicalized implementation workspace paths for isolated child environments, and routed validation commands using shell command substitution through `sh`. The direct Ratatui TUI run on `gannonh/uat-symphony` Project #16 published the issue #32 implementation preview with verified bundle metadata, no open pull request, and no `symphony/_32` remote branch. The UAT runbook now keeps typed implementation/review states out of legacy dispatch and uses the available Codex OAuth model.

## 2026-07-30
* **A3 PR2 shipped, A4 planned**: Roadmap and PRD mark A3 PR2 shipped in [#607](https://github.com/gannonh/kata-symphony/pull/607) (`d456c051`) and promote [A4 Agent Review Stage](/specs/2026-07-30-a4-agent-review-stage-design.md) as the next factory slice. Live AC20 UAT was not executed and is deferred by maintainer decision; the verify report stays Incomplete rather than claiming acceptance.

## 2026-07-29
* **A3 PR2 review remediation**: [#607](https://github.com/gannonh/kata-symphony/pull/607) pins publication identity, uses bounded/redacted token-authenticated Git subprocesses, verifies live PR state before Agent Review routing, and keeps retryable recovery pending. All original inline findings are addressed; automated gates Pass, with live AC20 UAT still residual.
* **A3 PR2 implemented**: Draft-PR publication / Agent Review handoff automation complete ([build](/specs/2026-07-29-a3-pr2-build-report.md); [verify](/specs/2026-07-29-a3-pr2-verify-report.md) Incomplete). Next: live UAT, then A4.
* **A3 PR1 next step = PR2**: Roadmap/PRD mark PR1 implemented ([#606](https://github.com/gannonh/kata-symphony/pull/606)); next factory slice is draft-PR publication / Agent Review handoff. Live PR1 UAT remains residual on the verify report only.
* **A3 PR1 implemented**: Preview path complete with [build](/specs/2026-07-29-a3-pr1-build-report.md) / [verify](/specs/2026-07-29-a3-pr1-verify-report.md) and [ADR-0004](/adrs/0004-a3-implementation-durability-and-bundles.md). Automated gates Pass; live UAT residual. PR2 is next.
* **A3 PR1 execution path**: Implemented validation cycles, content-addressed Git bundles, local credential-isolated runner (+ Docker env isolation builder), preview publisher, and implementation coordinator wired after A2 in `TriageRuntime::poll`. HTTP attach now surfaces bundle/publication fields. Automatic draft-PR publication remains PR2; full Docker container orchestration and live UAT remain residual.
* **A3 PR1 foundation**: Symphony implementation-stage module wired (domain/artifact/comment already present; store migration 004, SharedFactoryStore APIs, config/doctor/HTTP stubs, starter prompts, and WORKFLOW comments). Coordinator/runner/validation/bundle/publisher remain stubs for later PR1 slices.

## 2026-07-27
* **A1 PR3 shipped; A1 complete**: Roadmap marks A1 GitHub path complete (PR1–PR3, [#599](https://github.com/gannonh/kata-symphony/pull/599)), clears Blocked, and keeps A3 as next. Updated [docs index](/index.md), [specs roadmap](/specs/index.md), A1/A2/A3 designs, PRD, and PR3 build/re-verify reports.
* **A1 PR2 shipped; A3 next**: Roadmap and OKF entry points mark A1 PR2 shipped ([#598](https://github.com/gannonh/kata-symphony/pull/598)) and promote A3 to Active as the next factory slice. Updated [docs index](/index.md), [specs roadmap](/specs/index.md), A1/A2/A3 designs, and [factory PRD](/specs/symphony-software-factory-platform-prd.md).

## 2026-07-26
* **A3 draft**: Added the [A3 Implementation Stage](/specs/2026-07-26-a3-implementation-stage-design.md) on the shipped A2 approved-artifact boundary, covering credential-isolated local/Docker implementation, deterministic validation and repair, durable Git bundles, Symphony-owned draft-PR publication, and Agent Review handoff.
* **A2 completed**: Live GitHub UAT accepted for the durable GitHub spec stage ([verify report](/specs/2026-07-26-a2-uat-verify-report.md)). Nine product defects and two measure gaps found during UAT were fixed (`4fda65a1`, `17999528`). Roadmap, [A2 design](/specs/2026-07-18-a2-spec-stage-design.md), and [factory PRD](/specs/symphony-software-factory-platform-prd.md) mark A2 complete under the documented tracker-only narrowings.
* **A2 implementation**: Added the durable GitHub spec stage with isolated draft/review/revise turns, immutable versioned artifacts, tracker revision and approval decisions, pinned implement handoff, HTTP/metrics/events, doctor checks, starter prompts, and [ADR-0003](/adrs/0003-a2-spec-stage-artifacts-and-gates.md).

## 2026-07-25
* **A1 PR3 review threads resolved and CI passed**: [PR #599](https://github.com/gannonh/kata-symphony/pull/599) now isolates Codex process groups, retains unresolved orphan records, authorizes recursive cleanup against `workspace.root` and stage identity, and measures only latest publications using randomized bounded correction batches. All review threads and required CI checks pass; the [re-verify report](/specs/2026-07-25-a1-pr3-reverify-report.md) remains Incomplete pending credential rotation, UAT cleanup PR #18, and live verification.
* **A1 PR3 remediation automated gates passed; live re-verify blocked**: Stable PID/group/start-token recovery now permits launcher `exec`, UAT evidence records sanitized coordinates, cleanup validates all targets before provider work, and ten repeated library suites plus full validation pass. Added [ADR-0002](/adrs/0002-triage-process-recovery-identity.md) and an [incomplete re-verify report](/specs/2026-07-25-a1-pr3-reverify-report.md). Live UAT awaits provider credential rotation and merge of `gannonh/uat-symphony` cleanup PR #18.
* **A1 PR3 Verify rejected**: Added the [Verify report](/specs/2026-07-25-a1-pr3-verify-report.md) and updated the [roadmap](/specs/index.md), [A1 design](/specs/2026-07-16-a1-github-issue-triage-design.md), and [factory PRD](/specs/symphony-software-factory-platform-prd.md). Live correction/metrics checks passed; restart recovery leaked a post-`exec` orphan.

## 2026-07-24
* **A1 PR2 Verify accepted**: Live UAT on `gannonh/uat-symphony` Project #16 + automated suite; [verify report](/specs/2026-07-24-a1-pr2-verify-report.md). Roadmap/PRD/A1 design mark PR2 verified (merge pending).
* **A1 PR2 build**: Automatic route publication implemented; added [build report](/specs/2026-07-24-a1-pr2-build-report.md) and updated [specs roadmap](/specs/index.md), [A1 design](/specs/2026-07-16-a1-github-issue-triage-design.md), and [factory PRD](/specs/symphony-software-factory-platform-prd.md).

## 2026-07-18
* **Release model**: Switched to dispatch-driven releases (kata-code style). Symphony binary + Pi extension ship together at one version via `symphony-release.yml` (manual stable/nightly + 3h scheduled nightly). CLI stays independent via manual `cli-release.yml`. Removed push-to-main path-filter releases and standalone `pi-symphony-extension-release.yml`.
* **A1 PR1 shipped**: Updated [A1 design](/specs/2026-07-16-a1-github-issue-triage-design.md), [factory PRD](/specs/symphony-software-factory-platform-prd.md), and [specs roadmap](/specs/index.md) after [#587](https://github.com/gannonh/kata-symphony/pull/587); added [ADR-0001](/adrs/0001-a1-triage-durability-and-isolation.md) for triage durability and isolation decisions.

## 2026-07-17
* **A1 Preview**: Marked A1 triage preview implementation as active on the [specs roadmap](/specs/index.md); Symphony starter workflow reference documents storage/triage fields.

## 2026-07-16
* **A1 Draft**: Added the [GitHub Issue Triage design](/specs/2026-07-16-a1-github-issue-triage-design.md) and aligned the [triage label guide](/agents/triage-labels.md).
* **Roadmap**: Added the [Symphony Software Factory Platform PRD](/specs/symphony-software-factory-platform-prd.md) and promoted it in the OKF roadmap.

## 2026-07-15
* **Initialization**: Created OKF v0.1 bundle structure (`index.md`, `log.md`, `specs/`, `adrs/`, `guides/`, `domains/`).
* **Move**: Relocated Slack setup guide to [guides/slack-setup.md](/guides/slack-setup.md).
* **Index**: [specs/index.md](/specs/index.md) is the roadmap and links into existing [superpowers/](/superpowers/) plans and specs (left in place).
* **Seed**: Empty [adrs/](/adrs/) home for future architecture decisions; [domains/](/domains/) points at single-context domain-docs layout.
