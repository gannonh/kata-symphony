# Docs Update Log

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
