---
type: PRD
title: Symphony Software Factory Platform
status: Active
description: Product requirements and vertical-slice roadmap for evolving Symphony into a full software factory platform.
tags: [symphony, software-factory, roadmap, orchestration]
timestamp: 2026-07-25T22:35:00Z
---

# Symphony Software Factory Platform PRD

Status: Active
Date: 2026-07-16 (updated 2026-07-18)

Related: [A1 GitHub Issue Triage](/specs/2026-07-16-a1-github-issue-triage-design.md), [ADR-0001 A1 triage durability and isolation](/adrs/0001-a1-triage-durability-and-isolation.md), [Pi Symphony Extension Design](/superpowers/specs/2026-05-14-pi-symphony-extension-design.md), [Wave 4 Shared Context and Diagnostics Plan](/superpowers/plans/2026-05-17-wave-4-symphony-shared-context-diagnostics.md), [Specs roadmap](/specs/index.md)

## Executive summary

Symphony will evolve from an issue-driven agent orchestrator into a self-hostable software factory control plane. It will coordinate agents and humans across the complete delivery loop:

`intake → triage → spec → implement → review → verify → ship → monitor → intake`

A factory run will carry a work item through explicit stages, preserve the artifacts and decisions produced at each stage, enforce policy at privileged boundaries, and measure shipped outcomes against model, compute, and human cost.

Symphony already provides the execution nucleus: GitHub and Linear intake, isolated workspaces, Pi and Codex workers, state-specific prompts, retries, dependencies, steering, escalations, shared context, events, Docker and SSH execution, and operator surfaces. The roadmap extends those capabilities through user-facing vertical slices. Each pull request must deliver a workflow that a user can trigger, observe, and demonstrate.

## Research basis

This PRD applies four patterns from Warp's software factory series:

- A factory automates the SDLC loop while preserving steering, handoff, notifications, and human gates. See [A guide to cloud software factories for engineering leaders](https://www.warp.dev/blog/a-guide-to-cloud-software-factories-for-engineering-leaders).
- Triage routes work into implementation, specification, clarification, or parking. See [The automatic triage skill](https://www.warp.dev/blog/how-to-build-a-cloud-software-factory-the-automatic-triage-skill).
- Complex work produces product and technical specifications that humans approve, implementation consumes, and verification checks. See [Spec-driven development skills](https://www.warp.dev/blog/how-to-build-a-cloud-software-factory-add-spec-driven-development-skills).
- Review agents emit structured findings through a constrained publisher, and an outer loop proposes reviewed skill improvements from human feedback. See [Self-improving code review](https://www.warp.dev/blog/how-to-build-a-cloud-software-factory-self-improving-code-review).

The articles' reported automation rates are directional vendor claims. Symphony will establish its own baseline using shipped outcomes, quality, total cost, and human intervention.

## Product thesis

Engineering teams need a factory that:

- accepts work where it already originates;
- routes each work item according to ambiguity, risk, and verifiability;
- runs the appropriate agent, model, tools, and sandbox for each stage;
- keeps humans in control through explicit approvals, steering, and handoff;
- records what happened, why it happened, what it cost, and what shipped;
- learns through version-controlled, evaluated proposals;
- runs on customer-controlled compute and model providers.

Symphony can provide this through repository-owned factory configuration and a portable control plane built on its existing orchestration core.

## Users and jobs

### Engineering leader

- Understand how much accepted software the factory ships.
- Compare throughput, quality, human effort, and total cost.
- Find bottlenecks and decide where to expand automation.

### Factory operator or platform engineer

- Configure workflows, execution profiles, policies, integrations, and budgets as code.
- Observe active and historical runs across repositories.
- Drain workers, retry stages, answer escalations, and diagnose failures.

### Developer or reviewer

- Submit work through the tracker, chat, terminal, or monitoring systems.
- Review specs, diffs, findings, and verification evidence.
- Steer a live agent or take work into a local workspace and return it to the factory.

### Security or compliance owner

- Define which roles and agents may read, change, approve, merge, and deploy.
- Inspect immutable evidence linking intent, code, tests, approvals, and releases.
- Enforce model, data, secret, network, and retention policies.

## Goals

1. Close the full SDLC loop from intake through post-release monitoring.
2. Make every run durable, inspectable, resumable, and attributable.
3. Make human intervention fast through existing work surfaces.
4. Enforce least privilege and deterministic checks around privileged effects.
5. Support multiple harnesses, models, and execution providers.
6. Measure accepted delivery outcomes, quality, cost, and human effort.
7. Improve prompts, skills, routing, and policies through governed evaluation loops.
8. Keep factory configuration, memory, and improvement proposals owned by the customer.

## Scope boundaries

The initial platform will integrate with trackers, source forges, CI/CD, chat, and observability systems. It will preserve those systems as their domain sources of truth.

The initial platform will use a canonical software delivery loop. A general visual workflow builder and unrestricted DAG engine are deferred until multiple delivered workflows require them.

Autonomous merge and deployment will require explicit repository policy. Human approval remains the default for consequential changes.

Symphony will orchestrate model providers and agent harnesses. Model hosting and token resale are outside the product scope.

## Current Symphony baseline

### Capabilities to build on

- GitHub Issues/Projects v2 and Linear tracker adapters.
- Canonical lifecycle states from Backlog through Done.
- Priority, dependency, parent/child, retry, and concurrency-aware scheduling.
- Local, Docker, worktree, clone, and SSH worker execution.
- Pi and Codex harnesses; Pi supports state- and label-based model selection.
- State-specific prompts, repository-owned skills handled by the harness, lifecycle hooks, and a backend-neutral helper CLI.
- PR feedback/check inspection and merge-readiness helpers for GitHub.
- HTTP, Ratatui, and Pi operator surfaces.
- Versioned event envelopes, snapshots, structured logs, token accounting, and rate-limit data.
- Live steering, human escalation, Slack notifications, shared context, and supervisor rules.
- **GitHub A1 triage preview:** intake-label polling, SQLite factory/stage/artifact/publication durability, local isolated triage runner, marked preview comments, factory-run HTTP read APIs, doctor and starter workflow assets ([#587](https://github.com/gannonh/kata-symphony/pull/587)).

### Platform gaps

- Factory-run durability exists for **GitHub A1 triage preview** (SQLite runs, stage attempts, artifacts, publication intents, events, and minimal HTTP read APIs). Approvals, multi-stage history, Linear parity, and automatic route publication remain open.
- Specifications, reviews, verification, shipping, and monitoring are still primarily prompt conventions rather than typed stage outcomes. Triage is the first typed stage (preview mode only).
- Shared context and escalations are process-local.
- Remote HTTP operation lacks a complete authentication, authorization, and audit model.
- Runtime selection is service-wide and compute capacity is manually configured.
- Cost, human effort, merge/deploy outcomes, and quality are not joined into one scorecard.
- No experiment registry, evaluation promotion gate, or governed self-improvement loop exists.
- Production telemetry cannot yet create a linked factory work item and close the delivery loop.

## Product model

The roadmap uses this vocabulary. Storage and API contracts for A1 triage preview are recorded in [A1](/specs/2026-07-16-a1-github-issue-triage-design.md) and [ADR-0001](/adrs/0001-a1-triage-durability-and-isolation.md); later stages still need follow-up design and ADRs.

### Work item

The unit of desired change. It originates from a tracker, human request, schedule, support signal, or monitoring event.

### Factory run

The durable end-to-end record for one work item moving through the factory. It owns current stage, lineage, risk class, status, accumulated cost, and links to all stage runs and artifacts.

### Stage run

One bounded attempt at triage, spec, implementation, review, verification, shipping, or monitoring. It records inputs, execution profile, output, timing, usage, and result.

### Artifact

A typed, versioned output such as a triage decision, product spec, technical spec, patch, PR, review findings, verification manifest, approval, release, monitoring observation, or handoff bundle.

### Gate

A deterministic or human decision that controls a transition. Gates consume typed evidence and produce an auditable result.

### Execution profile

The harness, model, sandbox, tools, credentials, limits, and policies selected for a stage run.

### Signal

An external event that starts, resumes, or updates a factory run, such as an issue event, PR update, CI result, deployment, alert, schedule, or human response.

### Learning proposal

A version-controlled change to prompts, skills, routing, policy, or memory derived from measured factory feedback. It enters production through review, evaluation, and promotion gates.

## Target user journey

1. A user or system submits a work item.
2. Triage reproduces or assesses it and emits a route, rationale, confidence, risk class, and evidence.
3. Straightforward work enters implementation. Complex work enters specification. Ambiguous work requests human clarification. Deferred work remains parked with a reason.
4. Specification produces product behavior, technical shape, acceptance criteria, and open decisions. A human approves or revises it.
5. Implementation consumes the approved artifact version and opens a linked draft PR.
6. Review compares the PR with the codebase and approved specification, validates its findings, and publishes structured feedback through a constrained adapter.
7. Verification runs automated checks and user-facing acceptance workflows, then produces an evidence bundle.
8. Policy and human gates decide whether the change may merge and deploy.
9. Release signals are linked to the originating factory run.
10. Monitoring observes the released change. A validated regression creates a linked work item and begins the loop again.

At any stage, an authorized human can steer, answer an escalation, retry, park, cancel, or take over locally.

## Product principles

- **Factory as code:** repositories version workflow, prompt, skill, policy, and evaluation configuration.
- **Typed boundaries:** agents produce structured intents and evidence; deterministic services perform privileged mutations.
- **Governed autonomy:** risk, ambiguity, and objective verifiability determine routing and required approvals.
- **Portable execution:** harness, model, compute, and integration contracts remain replaceable.
- **Customer-owned knowledge:** artifacts, run history, memory, and evaluation data remain exportable and provider-neutral.
- **Evidence before status:** stage completion requires inspectable output, not an agent's success claim.
- **Human continuity:** steering, escalation, handoff, and notification remain available throughout the run.
- **Outcome economics:** optimization includes accepted delivery, quality, human time, inference, and compute.
- **Incremental delivery:** every pull request completes a real user journey and adds its own observability.

## Pull request delivery contract

Every feature pull request must include:

1. A real user or system trigger.
2. A user-visible result in at least one existing surface: tracker, PR, HTTP control room, Pi console, TUI, chat, or CLI.
3. An end-to-end path through the minimum domain, orchestration, integration, and presentation changes needed for that result.
4. A durable stage-run record and typed artifacts showing success and failure for every delivered stage slice.
5. Focused automated tests plus a short manual demo or UAT path.
6. Metrics or events that reveal adoption, latency, outcome, and failure.
7. Documentation for the delivered workflow.

Shared infrastructure must be pulled by a user-facing slice. Incomplete flows remain behind an explicit feature flag and cannot be presented as complete.

The roadmap horizons describe outcome clusters. They are not release trains or phase gates. A later slice may move earlier when it delivers higher validated value and its safety requirements are met.

## Vertical-slice roadmap

### Horizon A: close the first complete factory loop

#### A1. Issue intake → visible triage decision

A new GitHub or Linear issue triggers a triage stage. Symphony creates the minimum durable factory run, triage stage run, and triage artifact for that issue. The issue receives one route: implement, spec, needs information, park, or human-owned. The tracker shows the rationale, evidence, risk class, and next action.

**Progress (2026-07-27):** GitHub path complete — preview [#587](https://github.com/gannonh/kata-symphony/pull/587), automatic route publication [#598](https://github.com/gannonh/kata-symphony/pull/598), recovery and agreement [#599](https://github.com/gannonh/kata-symphony/pull/599). Design: [A1 GitHub Issue Triage](/specs/2026-07-16-a1-github-issue-triage-design.md). Decisions: [ADR-0001](/adrs/0001-a1-triage-durability-and-isolation.md), [ADR-0002](/adrs/0002-triage-process-recovery-identity.md).

| Slice | Status | Notes |
| --- | --- | --- |
| PR1 preview | **Shipped** | Intake label + Projects v2 membership, local Pi/Codex triage, immutable artifact, durable preview comment, factory-run HTTP read API, doctor/starter assets |
| PR2 automatic route publication | **Shipped** | [#598](https://github.com/gannonh/kata-symphony/pull/598); apply route labels/states, remove intake label, implement handoff ([build](/specs/2026-07-24-a1-pr2-build-report.md), [verify](/specs/2026-07-24-a1-pr2-verify-report.md)) |
| PR3 recovery and agreement | **Shipped** | [#599](https://github.com/gannonh/kata-symphony/pull/599); post-`exec` recovery, process-group isolation, retained unresolved recovery state, cleanup authorization, correction measurement ([build](/specs/2026-07-25-a1-pr3-build-report.md), [re-verify](/specs/2026-07-25-a1-pr3-reverify-report.md); historical [rejected verify](/specs/2026-07-25-a1-pr3-verify-report.md)) |
| Linear triage | Deferred | Separate vertical slice after GitHub path is complete |

**Demo (PR1):** Label a project-member issue `needs-triage`. Symphony records a factory run, posts a marked preview comment with route/rationale/evidence, and exposes the run over `GET /api/v1/factory-runs`. Off-project intake gets a diagnostic comment without an agent attempt.

**Measure:** routing agreement with human review, triage latency, clarification rate, and cost per decision.

#### A2. Triage → approved product and technical specification

A spec-routed issue produces versioned product behavior, technical approach, acceptance criteria, and open decisions. A human can approve or request revision from the tracker or Pi.

**Progress:** Completed for the narrowed GitHub tracker workflow in [A2 Spec Stage](/specs/2026-07-18-a2-spec-stage-design.md) ([UAT verify](/specs/2026-07-26-a2-uat-verify-report.md); [ADR-0003](/adrs/0003-a2-spec-stage-artifacts-and-gates.md)): isolated draft/review/revise turns, durable immutable versions, GitHub feedback/decision labels, approved-version pinning, implement label/state handoff, HTTP artifacts/metrics (including review-cycle, convergence, revision-request, and approval-latency aggregates), Pi token capture, events, doctor checks, and starter assets. Tracker approval is delivered; Pi approval and separate product/technical artifacts remain deferred as documented A2 narrowings.

**Demo:** Apply or receive the spec route, review both artifacts, request one change, approve the revision, and see the run become implementation-ready.

**Measure:** approval cycles, time awaiting human input, implementation rework attributed to spec gaps.

#### A3. Approved specification → linked draft PR

Implementation consumes the approved artifact version, runs repository validation, and opens a draft PR that links the issue, spec version, factory run, and validation summary.

**Progress (2026-07-29):** **PR1 implemented** in [#606](https://github.com/gannonh/kata-symphony/pull/606) ([build](/specs/2026-07-29-a3-pr1-build-report.md), [verify](/specs/2026-07-29-a3-pr1-verify-report.md), [ADR-0004](/adrs/0004-a3-implementation-durability-and-bundles.md)). **PR2 shipped** in [#607](https://github.com/gannonh/kata-symphony/pull/607) (`d456c051`) ([build](/specs/2026-07-29-a3-pr2-build-report.md), [verify](/specs/2026-07-29-a3-pr2-verify-report.md) Incomplete — live AC20 UAT deferred by maintainer decision, not executed). Design: [A3 Implementation Stage](/specs/2026-07-26-a3-implementation-stage-design.md). A4 proceeds without the live A3 UAT.

| Slice | Status | Notes |
| --- | --- | --- |
| PR1 implementation and validation preview | **Implemented** ([#606](https://github.com/gannonh/kata-symphony/pull/606)) | Local preview path; Docker profile fail-closes pending full container orchestration; live UAT residual |
| PR2 deterministic draft-PR publication | **Shipped** ([#607](https://github.com/gannonh/kata-symphony/pull/607)) | Expected-projection branch push, owned draft PR, restart recovery, Agent Review handoff, bounded publication retries; live UAT deferred |

**Demo:** Approve a spec and watch Symphony produce a draft PR whose description records the intended behavior and evidence.

**Measure:** draft-PR success rate, implementation cycle time, retries, human interventions, and cost.

#### A4. Draft PR → structured, read-only agent review

A review stage consumes the PR description, diff, repository context, and approved spec. It emits schema-validated findings. A deterministic publisher creates PR comments using narrow credentials.

**Demo:** Open or update a PR, receive a review summary and inline findings, push a correction, and see contextual re-review.

**Measure:** accepted finding rate, dismissed finding rate, review cycles, escaped sampled defects, and review cost.

#### A5. Reviewed PR → verification evidence

A verification stage runs configured tests and one user-facing acceptance path appropriate to the product. It stores commands, results, artifacts, and spec-conformance evidence, then advances the work to human review when required gates pass.

**Demo:** Show a passing change with evidence and a failing change held at the gate with a concrete reason.

**Measure:** verification pass rate, flaky reruns, spec-conformance failures, escaped defects, and verification cost.

#### A6. Approved change → governed merge and deployment

Symphony evaluates required checks, reviews, verification, risk policy, and human approval before invoking the forge's merge operation. One configured CI/CD integration then deploys the merged change and records the terminal deployment or release outcome on the factory run.

**Demo:** Merge and deploy a qualified change, then deny a change missing one required artifact and show that it remains undeployed. Show both decisions and the release record in the control surface.

**Measure:** merge lead time, policy denials, change failure rate, deployment frequency, and rollback rate.

#### A7. Release signal → linked follow-up issue

Symphony accepts one production signal source, correlates it with a release when possible, and lets a monitoring stage create a linked issue with evidence. The issue enters triage automatically.

**Demo:** Send a simulated regression event, observe a linked issue, and watch it enter the same factory loop.

**Measure:** valid signal rate, duplicate suppression, detection-to-triage time, and failed-deployment recovery time.

### Horizon B: operate the factory as a remote platform

#### B1. Cross-restart recovery and complete run timeline

Use the durable records delivered by each stage slice to make active runs recoverable after restart and present a complete operator timeline. The HTTP and Pi surfaces show current stage, prior attempts, artifacts, approvals, spend, and required human action.

**Demo:** Restart Symphony during an awaiting-approval run, reopen the complete timeline, approve it, and observe the same factory run continue.

**Measure:** recovery success, orphaned runs, timeline query latency, and operator diagnosis time.

#### B2. Authenticated remote observation and control

Protect HTTP and WebSocket surfaces with identities and roles for read, operate, approve, and administer. Record every control action in the run timeline.

**Demo:** A viewer can inspect a run but cannot steer it; an operator steers it and the audit trail records the action.

**Measure:** denied actions, authentication failures, privileged actions, and audit completeness.

#### B3. Chat escalation → authenticated response

Send actionable escalations to one chat integration and accept an authenticated response that resumes the waiting stage.

**Demo:** A worker asks a multiple-choice question, a human answers in chat, and the same stage continues.

**Measure:** response time, timeout rate, channel adoption, and resumed-stage success.

#### B4. Cloud run → local handoff → cloud resume

Package the workspace reference, session summary, artifacts, pending decision, and run identity into a handoff command. A developer works locally and returns an explicit result to the factory.

**Demo:** Take over a stuck run locally, commit a correction, and resume review under the same factory run.

**Measure:** handoff success, context reconstruction failures, local intervention time, and resumed-run outcome.

#### B5. Worker health and drain controls

Show local, Docker, and SSH worker capacity, health, active runs, and recent failures. Operators can drain a worker without disrupting completed artifacts.

**Demo:** Drain one SSH host and observe new work route to another eligible host.

**Measure:** scheduling latency, failed placements, worker utilization, drain time, and host error rate.

#### B6. One ephemeral cloud runtime provider

Launch a stage as an ephemeral cloud job through a provider contract, with explicit image, resource, secret, and network policy. Display provisioning and teardown in the run timeline.

**Demo:** Route one labeled issue to the cloud provider and show its isolated lifecycle from launch through artifact collection and deletion.

**Measure:** startup latency, teardown success, cost, provider failures, and policy violations.

### Horizon C: govern autonomy and factory economics

#### C1. Risk class → policy and approval gate

Assign risk from tracker metadata and triage evidence. Configure which stages, tools, and approvals each risk class requires.

**Demo:** A documentation change follows a lightweight path while an authentication change requires security approval and stronger verification.

**Measure:** work by risk class, approval wait, overrides, post-merge incidents, and policy accuracy.

#### C2. Stage-scoped credentials and effect adapters

Issue each stage only the credentials and typed capabilities it needs. Privileged mutations such as comments, state changes, merge, and deployment pass through deterministic policy checks.

**Demo:** A review agent can propose comments but cannot mutate another PR or merge code.

**Measure:** denied effects, credential scope, secret exposure findings, and policy adapter errors.

#### C3. Budget → model, harness, and compute routing

Set budgets by repository, risk class, stage, or work item. Resolve an execution profile from measured quality, latency, availability, data policy, and remaining budget. Pause or escalate before exceeding a hard limit.

**Demo:** Two issue classes select different profiles; a budget threshold pauses a costly retry and asks an operator.

**Measure:** cost per accepted change, budget variance, route distribution, quality by profile, and provider availability.

#### C4. Release evidence and provenance bundle

Export a signed bundle linking work item, approved spec, source commit, execution profiles, review, verification, approvals, build, release, and monitoring observation.

**Demo:** Open a completed run and download one evidence bundle that verifies its artifact lineage.

**Measure:** provenance completeness, verification failures, missing links, and export latency.

#### C5. Portfolio factory scorecard

Aggregate repositories into an organization view with throughput, quality, cost, intervention, stage bottlenecks, risk, and worker capacity.

**Demo:** Compare two repositories and drill from an outlier metric to the contributing runs.

**Measure:** scorecard usage, data freshness, unattributed cost, and time to identify a bottleneck.

### Horizon D: build a learning factory

#### D1. Factory evaluation dataset and scorecard

Export reviewed factory runs into a versioned evaluation dataset with stage inputs, outputs, human judgments, quality outcomes, and cost. Run one repeatable evaluation from the CLI and show results in the control room.

**Demo:** Evaluate the current triage or review profile against representative historical cases and inspect failures.

**Measure:** dataset coverage, grader agreement, regression rate, evaluation cost, and result reproducibility.

#### D2. Execution profile experiment

Assign a bounded cohort to two approved profiles and compare quality, latency, cost, and intervention without changing unrelated stages.

**Demo:** Run a review-profile experiment, stop it at the configured sample boundary, and select a winner from recorded evidence.

**Measure:** experiment completion, outcome delta, guardrail violations, and decision confidence.

#### D3. Durable explicit memory proposal

Let humans submit durable factory guidance. A proposal shows scope, source, affected stages, and conflicts, then enters review before becoming retrievable context.

**Demo:** Record a repository convention, approve it, and show the next relevant stage cite and apply it.

**Measure:** memory acceptance, retrieval precision, conflicts, corrections, and stale-memory removals.

#### D4. Human review feedback → skill improvement PR

An outer-loop stage analyzes accepted and rejected review findings, proposes a version-controlled skill change, runs the review evaluation set, and opens a PR with before/after evidence.

**Demo:** Correct a review pattern, run the improvement loop, and inspect a proposed skill PR that passes evaluation without auto-merging.

**Measure:** proposal acceptance, evaluation improvement, regressions, reviewer correction rate, and rollback rate.

#### D5. Automation coverage recommendation

Analyze parked, handed-off, retried, and successful runs to recommend one bounded workflow or task class suitable for greater automation. Recommendations include expected value, evidence, risk, and the next evaluation.

**Demo:** Open a recommendation, approve a canary policy change, and observe only the selected task class use it.

**Measure:** recommendation acceptance, autonomous completion by task class, quality guardrails, and human minutes saved.

## Current Wave 4 disposition

The active Pi extension Wave 4 plan contributes two useful platform slices:

1. Shared-context viewing and mutation creates an operator-visible knowledge workflow.
2. Polling, token, rate-limit, and event diagnostics create a factory operations workflow.

Ship these as independent, demoable pull requests under the delivery contract above. Durable context belongs in Horizon D3, while durable run diagnostics and history belong in Horizon B1. The larger roadmap becomes the organizing product direction for Symphony.

## Architecture direction

### Control plane

A durable run coordinator owns factory runs, stage state, retries, gates, approvals, schedules, idempotency, and recovery. Tracker state remains synchronized as a user-facing workflow projection.

### Execution plane

Execution providers launch stage runs in local, Docker, SSH, and cloud sandboxes. Execution profiles carry harness, model, image, tools, identity, resources, network policy, and limits.

### Artifact and knowledge plane

Typed artifacts retain specs, patches, review findings, verification evidence, release records, memory, and handoff bundles. Stable run and artifact identifiers connect tracker, source, CI/CD, and monitoring records.

### Integration plane

Typed adapters receive signals and perform narrow effects for trackers, source forges, CI/CD, chat, observability, and incident systems.

### Policy and trust plane

Authentication, authorization, risk policy, approvals, secret issuance, audit, retention, and provenance apply consistently across control and execution planes.

### Evaluation plane

Versioned datasets, graders, experiments, scorecards, and promotion gates compare prompts, skills, models, harnesses, and policies.

### Existing extension seams

- `apps/symphony/src/domain.rs`: work item, lifecycle, event, snapshot, configuration, harness, escalation, and context contracts.
- `apps/symphony/src/orchestrator.rs`: dispatch, retry, reconciliation, stage transition, and worker execution flow.
- `apps/symphony/src/linear/adapter.rs` and `apps/symphony/src/github/adapter.rs`: tracker input and mutation ports.
- `apps/symphony/src/event_stream.rs`: event source for durable run history and external signals.
- `apps/symphony/src/http_server.rs`: integration and operator control surface; authentication precedes wider exposure.
- `apps/symphony/src/workspace.rs`, `docker.rs`, and `ssh.rs`: starting point for execution-provider contracts.
- `apps/symphony/src/shared_context.rs`: starting point for durable, scoped knowledge.
- `apps/symphony/src/helper.rs`: starting point for typed privileged effects.

## Success measures

### North star

**Verified shipped outcomes per total factory cost**, where:

- a shipped outcome is an accepted change that passed required verification and remained within configured post-release quality guardrails;
- total factory cost includes inference, compute, storage, integrations, and attributed human review time.

### Flow

- Intake-to-triage, triage-to-approved-spec, issue-to-draft-PR, PR lead time, deployment frequency, and queue time by stage.

### Quality

- Change failure rate, deployment rework rate, escaped defects, rollback rate, security findings, spec-conformance rate, and sampled review precision.

### Automation and human effort

- Completion without intervention by task and risk class, handoff rate, escalation rate, retries, and human minutes per shipped outcome.

### Economics

- Cost per triage, spec, accepted review finding, verified PR, merged change, and deployed outcome; budget variance by profile and repository.

### Reliability

- Stage success, provider error, stall and timeout rate, sandbox startup latency, recovery success, and mean time to operator intervention.

### Safety

- Denied privileged effects, policy overrides, credential violations, missing provenance, audit gaps, and incidents attributable to factory changes.

Initial numeric targets will be set after a dogfood baseline. Before autonomous merge or deployment, or expansion to another task class, the team must publish the baseline window or sample, measured baseline, and explicit quality, safety, reliability, and cost guardrails. Those guardrails remain fixed while throughput and cost are optimized.

## Rollout and learning strategy

1. Dogfood one repository and one low-risk task class.
2. Declare the baseline window or sample, then publish existing issue-to-PR flow, quality, reliability, intervention, and cost measures before changing routing.
3. Define numeric guardrails from that baseline before enabling autonomous merge or deployment.
4. Introduce one typed stage outcome at a time and retain human approval.
5. Expand eligible task classes only when a completed baseline comparison satisfies the published guardrails.
6. Canary new execution profiles, prompts, skills, policies, and memory against a bounded cohort.
7. Promote changes through version-controlled review and evaluation evidence.
8. Preserve manual routing, steering, handoff, pause, and rollback throughout rollout.

## Risks and responses

### Workflow engine overreach

Start with the canonical software delivery loop and optional stages. Add generalized branching only when delivered slices reveal repeated requirements.

### Policy split between prompts and runtime

Represent consequential gates and effects as typed runtime policy. Prompts explain the policy and produce evidence for it.

### Tracker and control-plane race conditions

Give each factory run a stable identity, make transitions idempotent, record expected prior state, and reconcile user-visible tracker state from durable events.

### Prompt injection and excessive authority

Treat issue, repository, PR, chat, and tool content as untrusted. Use stage-scoped identities, read/write separation, deterministic effect adapters, sandbox policy, secret redaction, and human confirmation for consequential actions.

### Fabricated or incomplete evidence

Collect verification results and artifacts from trusted runners and deterministic adapters. Record the exact command, environment, commit, and artifact digest.

### Memory poisoning and self-modification

Store provenance and scope for every durable memory item. Route learning through a proposal PR, evaluation, approval, canary, and rollback.

### Metric gaming

Combine throughput with quality, human effort, cost, and post-release outcomes. Drill every aggregate metric back to run evidence.

### Cloud operational complexity

Build remote operation from current Docker and SSH behavior, then add one execution provider through a user-facing workload before broad provider abstraction.

## Architecture decisions required

Create ADRs when implementation reaches these decisions:

1. Durable factory-run and artifact storage.
2. Canonical stage and recipe representation.
3. Tracker state synchronization and conflict resolution.
4. Authentication, authorization, tenant, and audit boundaries.
5. Execution-provider contract and workload identity.
6. Typed artifact schemas and provenance format.
7. Evaluation dataset, grader, and promotion model.
8. Durable memory scope, ownership, conflict, and retention rules.

## Open product questions

1. Which deployment model leads: fully self-hosted, managed control plane with customer execution, or both?
2. Is GitHub the first complete forge/CI/release integration while Linear remains a tracker, or must all slices launch with backend parity?
3. Which product surface should become the primary control room: web, Pi, or a shared model rendered by both?
4. Which production signal should close the first loop: GitHub deployments, Sentry, Datadog, OpenTelemetry, or a generic webhook?
5. How will human time be captured with enough accuracy for factory economics?
6. Which risk classes and privileged actions require mandatory human approval in the first policy model?
7. When should multi-repository workflows and organization tenancy enter the roadmap?

## Platform acceptance criteria

Symphony qualifies as the intended software factory platform when:

- a work item can traverse triage, optional spec, implementation, review, verification, governed ship, and monitoring;
- every stage produces durable, inspectable artifacts and usage data;
- humans can approve, steer, escalate, hand off, retry, park, and cancel through authenticated surfaces;
- execution can run on at least one ephemeral remote provider with scoped identity and policy;
- operators can inspect current and historical runs, bottlenecks, spend, and required action;
- leaders can evaluate accepted delivery, quality, human effort, and total cost;
- factory changes and memory updates enter production through version-controlled evaluation and approval;
- a post-release signal can create a linked work item and restart the loop.
