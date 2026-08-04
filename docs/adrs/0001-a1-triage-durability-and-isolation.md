---
type: ADR
title: A1 triage durability and isolation
status: Accepted
description: Durable SQLite factory runs, forge-host identity, Projects v2 membership keys, and local runner isolation for A1 GitHub triage preview.
tags: [symphony, triage, adr, sqlite, security]
timestamp: 2026-07-18T16:45:00Z
---

# ADR-0001: A1 triage durability and isolation

## Status

Accepted (2026-07-18) with A1 PR1 preview ([#587](https://github.com/gannonh/kata-symphony/pull/587)).

## Context

A1 introduces a factory stage before implementation. Maintainers need restart-safe triage decisions, visible preview comments, and a runner that cannot mutate the operator's GitHub credentials or host home while still authenticating Pi/Codex. The full design lives in [A1 GitHub Issue Triage](/specs/archive/2026-07-16-a1-github-issue-triage-design.md); this ADR records the durable constraints shipped in preview.

## Decision

1. **SQLite factory-run store** — Persist factory runs, stage attempts, immutable artifacts, publication intents, and events in a namespaced SQLite database before and after agent execution. Default path is platform data dir under `symphony/triage/<forge>/<owner>/<repo>/`.

2. **Forge identity vs API endpoint** — Storage namespaces and issue URLs use the web forge host. Map public GitHub API host `api.github.com` to `github.com` so doctor checks and runtime agree.

3. **Projects v2 membership key** — Intake treats an issue as in-project only when both repository (`owner/repo`) and issue number match a project item. Issue numbers alone are not unique across multi-repo projects.

4. **Local clone-only runner** — Triage runs in a disposable workspace clone with cleared environment, isolated `HOME`, no injected `GH_TOKEN`/`GITHUB_TOKEN`/helper env, and push URLs disabled. Seed Pi auth into the isolated home only for the turn, then scrub that home before retaining successful evidence (fail closed if scrub fails).

5. **Lease renewal** — Stage leases renew on a fixed interval while the turn runs so concurrent polls do not interrupt in-flight attempts after the stale threshold (60s) while turn timeout remains much longer (default 900s).

6. **Preview publication intents** — Successful artifacts always have a matching preview publication intent. If intent creation is interrupted after artifact store, the next poll recreates the intent and publishes rather than skipping forever.

7. **Preview mode effects** — Preview writes an idempotent marked comment only; it does not change route labels, project state, or remove the intake label. Automatic publication remains PR2.

## Consequences

- Operators can restart Symphony and reconcile pending preview comments from durable state.
- Multi-repo GitHub Projects do not false-positive triage eligibility by issue number alone.
- Codex and Pi triage share the same env-isolation policy; credentials are not left on disk after success.
- Automatic routing, implement handoff, and agreement metrics are intentionally deferred to later A1 slices.

## Links

- Spec: [A1 GitHub Issue Triage](/specs/archive/2026-07-16-a1-github-issue-triage-design.md)
- PRD: [Symphony Software Factory Platform](/specs/archive/symphony-software-factory-platform-prd.md)
- Labels: [Triage labels](/agents/triage-labels.md)
- Implementation: `apps/symphony/src/triage/`, `apps/symphony/src/doctor.rs`, `apps/symphony/docs/WORKFLOW-REFERENCE.md`
