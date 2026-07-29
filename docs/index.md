---
okf_version: "0.1"
---

# Kata monorepo knowledge

Open Knowledge Format (OKF) bundle for the Kata monorepo: Kata CLI (`apps/cli`) and Kata Symphony (`apps/symphony`).

# Roadmap

* [Symphony Software Factory Platform PRD](specs/symphony-software-factory-platform-prd.md) - product direction and vertical-slice roadmap (Active; A1+A2 completed; A3 PR1 implemented — draft-PR publication is PR2)
* [Specs roadmap](specs/) - active, planned, and completed work (links into historical Superpowers plans/specs)
* [A3 Implementation Stage](specs/2026-07-26-a3-implementation-stage-design.md) - Active; PR1 implemented ([#606](https://github.com/gannonh/kata-symphony/pull/606)); **next** PR2 draft-PR publication
* [A1 GitHub Issue Triage](specs/2026-07-16-a1-github-issue-triage-design.md) - completed GitHub path (PR1–PR3 shipped: [#587](https://github.com/gannonh/kata-symphony/pull/587), [#598](https://github.com/gannonh/kata-symphony/pull/598), [#599](https://github.com/gannonh/kata-symphony/pull/599)); Linear triage deferred
* [A2 Spec Stage](specs/2026-07-18-a2-spec-stage-design.md) - completed; live UAT accepted ([verify](specs/2026-07-26-a2-uat-verify-report.md))
* [ADRs](adrs/) - architecture decision records ([ADR-0001](adrs/0001-a1-triage-durability-and-isolation.md), [ADR-0002](adrs/0002-triage-process-recovery-identity.md), [ADR-0003](adrs/0003-a2-spec-stage-artifacts-and-gates.md), [ADR-0004](adrs/0004-a3-implementation-durability-and-bundles.md))

# Guides

* [Guides](guides/) - setup and operational how-tos
* [Agent skills config](agents/) - issue tracker, triage labels, domain-docs layout for skills

# Domains

* [Domains](domains/) - product/domain vocabulary (single-context layout; `CONTEXT.md` created lazily)

# Historical plans and designs

* [Superpowers specs](superpowers/specs/) - design specs (active + `_archive`)
* [Superpowers plans](superpowers/plans/) - implementation plans (active + `_archive`)

# Product entry points (outside this bundle)

Paths relative to the repo root (not OKF concept links):

* `README.md` — products overview
* `apps/cli/README.md` — Kata CLI
* `apps/symphony/README.md` — Kata Symphony
* `AGENTS.md` — agent operating instructions for this repo
