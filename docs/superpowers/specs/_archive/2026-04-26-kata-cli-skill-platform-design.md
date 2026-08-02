---
type: Spec
title: Kata Cli Skill Platform Design
description: "Archived spec document: Kata Cli Skill Platform Design."
tags: [archive, cli]
timestamp: 2026-07-15T20:00:00Z
---

# Kata CLI Skill Platform Design

**Date:** 2026-04-26
**Status:** Draft
**Scope:** Replace the current Pi-coupled Kata CLI and supersede `apps/orchestrator` with a skill-first, harness-portable Kata platform backed by a typed Node CLI domain API

## 1) Problem

Kata CLI currently serves two jobs at once:

1. a custom coding-agent runtime built on top of `@mariozechner/pi-coding-agent`
2. Kata's planning/execution product surface

That coupling now creates four structural problems:

1. Kata workflow behavior is tied to the Pi runtime rather than to a portable skill layer.
2. Backend operations, artifact storage, and workflow semantics are mixed into a custom CLI distribution instead of a reusable backend contract.
3. Desktop relies on the custom Kata CLI wrapper for RPC mode even though Pi itself is the actual agent runtime.
4. `apps/orchestrator` already models Kata as a skill-driven planning/execution product, but it is file-centric and does not use a canonical backend abstraction suitable for GitHub/Linear-backed execution.

## 2) Goal

Create a new Kata architecture with these properties:

1. Kata's planning and execution product is expressed primarily as a reusable skill suite targeting the published Skills spec.
2. A standalone Node CLI exposes a typed Kata domain API for all durable workflow operations.
3. GitHub and Linear are fully abstracted behind the same canonical contract.
4. Desktop uses Pi directly in RPC mode and uses the Kata CLI as a separate workflow/backend layer.
5. Symphony remains the autonomous execution layer and continues polling backend state directly.

## 3) Non-goals

1. No backward-compatibility layer for legacy Kata CLI behavior so long as planned artifacts remain unchanged.
2. No GitHub label-based workflow mode. GitHub support is Projects v2 only.
3. No alternate prompt/skill suites per backend.
4. No change to Symphony's core polling-and-dispatch execution model.
5. No product/runtime-owned release orchestration. CI owns packaging, publishing, and distribution workflows.

## 4) Locked Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Primary product surface | Skills | Matches portability goal and published Skills standard |
| Durable workflow/backend layer | Node CLI with typed Kata domain API | Centralizes state, backend abstraction, auth, retries, and external side effects |
| Backend abstraction stance | Strict, normative contract | Backend-specific quirks are unacceptable above adapter internals |
| GitHub support | Projects v2 only | Removes label-mode complexity and fallback branches |
| Desktop agent runtime | Pi directly in RPC mode | Removes unnecessary dependency on custom Kata CLI wrapper |
| Desktop distribution role | Purpose-built integrated Kata product | Bundles Pi RPC runtime, Kata skills, Kata CLI, and Symphony binaries |
| Symphony coordination model | Backend-polled, not handoff-based | Matches current daemon model and avoids an unnecessary second protocol |
| Setup path | `npm` / `npx @kata-sh/cli setup` plus harness packaging | Gives one universal bootstrap path and harness-native install options |
| Release/distribution owner | CI | Keeps runtime concerns separate from publishing concerns |

## 5) High-Level Architecture

```
Harness / Desktop
    |
    | installs and invokes
    v
Kata Skills (canonical workflow layer)
    |
    | calls stable typed operations
    v
Kata CLI (Node domain/backend layer)
    |
    +--> GitHub Projects v2 adapter
    +--> Linear adapter
    +--> Symphony operational adapter
```

The defining rule is:

1. Skills own workflow intelligence and user-facing Kata behavior.
2. The CLI owns durable state and external side effects.
3. Harness adapters own packaging/integration only.
4. Desktop owns the integrated user experience, not the workflow contract.

## 6) Component Responsibilities

### 6.1 Harness-agnostic Kata skills

The skill suite is the Kata product.

Responsibilities:

1. implement workflows such as `new-project`, `discuss-phase`, `plan-phase`, `execute-phase`, `verify-work`, `pr`, `quick`, and setup/init flows
2. define the canonical Kata user experience and workflow sequencing
3. express planning, execution, review, and verification behavior in a backend-neutral way
4. call the CLI only through stable domain operations

Non-responsibilities:

1. no GitHub- or Linear-specific logic
2. no backend-specific artifact layout assumptions
3. no persistence logic beyond natural harness/session context
4. no release/distribution behavior

### 6.2 Node-based Kata CLI

The CLI becomes a standalone Node utility and the canonical workflow backend layer.

Responsibilities:

1. expose the typed Kata domain API
2. own all durable state access and external side effects
3. implement backend adapters for GitHub Projects v2 and Linear
4. resolve auth, retries, pagination, normalization, validation, and rate-limit handling
5. provide setup/bootstrap flows for harness installation and configuration
6. provide operational commands such as doctor/setup/inspection for interactive and non-harness use

Non-responsibilities:

1. no custom coding-agent runtime behavior
2. no ownership of the main Kata product UX
3. no harness-specific workflow forks

### 6.3 Harness-specific adapters / plugin flavors

The skills are uniform because they target a published Skills spec. Variation is allowed only in packaging and integration.

Responsibilities:

1. install or register the canonical Kata skills in the host harness
2. expose the CLI entrypoint in the harness's preferred way
3. apply harness-specific config conventions, file locations, hooks, and manifest/plugin structure
4. integrate harness-specific publishing/install expectations

Allowed variation:

1. config files and schema shape
2. install locations
3. plugin manifests
4. hook registration
5. packaging and publishing mechanics

Forbidden variation:

1. workflow semantics
2. backend behavior
3. domain object definitions
4. planning/execution/review logic

### 6.4 Desktop app

Desktop becomes the purpose-built integrated Kata product.

Responsibilities:

1. use `@mariozechner/pi-coding-agent` directly in RPC mode for the conversational runtime
2. bundle prepackaged Kata skills
3. bundle the Kata CLI
4. bundle Symphony binaries
5. use the Kata CLI as the workflow/backend layer for planning boards, artifacts, and workflow-domain actions

This means Desktop is not "another harness flavor." It is the integrated packaged Kata environment built around the same canonical skills and CLI contract.

### 6.5 Symphony

Symphony remains the autonomous execution daemon.

Responsibilities stay unchanged in principle:

1. poll backend state for dispatchable work
2. select eligible `Todo` work
3. dispatch Pi coding-agent workers in worktrees
4. continue to observe backend state directly

Allowed change:

1. targeted updates if Kata-facing skill names, tool names, or interface expectations change

Not allowed:

1. new bespoke execution handoff payloads if backend state already serves as the protocol

## 7) Domain Contract

The Kata domain model is authoritative. Backend adapters must conform to it exactly.

If a backend cannot faithfully satisfy the contract, that adapter is incomplete and must be fixed. No GitHub- or Linear-specific semantics may leak into:

1. skills
2. desktop workflow behavior
3. Symphony-facing workflow semantics
4. higher-level product surfaces

### 7.1 Core domain objects

The CLI should normalize all backend behavior into the following core objects:

1. `Project`
2. `Milestone`
3. `Slice`
4. `Task`
5. `Artifact`
6. `PullRequest`
7. `ExecutionStatus` or similar operational read models for Symphony visibility, if needed

Suggested object expectations:

- `Project`: backend type, repo/workspace metadata, active tracker configuration
- `Milestone`: `id`, `title`, `goal`, `status`, `active`, ordering metadata
- `Slice`: `id`, `milestoneId`, `title`, `goal`, `status`, ordering, review state
- `Task`: `id`, `sliceId`, `title`, `description`, `status`, verification/review metadata
- `Artifact`: `id`, `scopeType`, `scopeId`, `artifactType`, `title`, `content`, `format`, `updatedAt`, provenance metadata
- `PullRequest`: `id`, `url`, `branch`, `base`, `status`, checks, merge readiness

### 7.2 API surface

The API should be grouped around Kata domains rather than backend transport details.

Representative surface:

1. `project.getContext()`
2. `milestone.getActive()`, `milestone.list()`, `milestone.create()`, `milestone.complete()`
3. `slice.list()`, `slice.create()`, `slice.update()`, `slice.reorder()`
4. `task.list()`, `task.create()`, `task.update()`, `task.move()`, `task.complete()`
5. `artifact.list()`, `artifact.read()`, `artifact.write()`
6. `pr.open()`, `pr.getStatus()`, `pr.requestReview()`, `pr.merge()`
7. `execution.getDispatchableWork()`, `execution.getWorkerStatus()`, `execution.getEscalations()` if operational visibility needs to be surfaced through the CLI
8. `setup.detectHarness()`, `setup.install()`, `setup.doctor()`

### 7.3 Artifact model

Artifacts remain first-class but backend-abstracted.

Recommended canonical artifact types:

1. `project-brief`
2. `requirements`
3. `roadmap`
4. `phase-context`
5. `research`
6. `plan`
7. `summary`
8. `verification`
9. `uat`
10. `retrospective`

The contract guarantees:

1. `artifact.read/write/list` semantics are identical across backends
2. storage layout decisions remain adapter-internal
3. no skill may care how an artifact is stored in GitHub or Linear

## 8) Backend Adapters

### 8.1 General rule

Adapters are allowed to differ internally. They are not allowed to differ at the contract boundary.

That means:

1. backend-specific normalization happens inside the adapter
2. missing backend capabilities must be emulated or composed until they satisfy the canonical contract
3. backend-specific quirks are a defect in the adapter if they appear above the CLI boundary

### 8.2 Linear adapter

Responsibilities:

1. map the canonical Kata domain model onto Linear projects, issues, sub-issues, and documents
2. support the same milestone/slice/task/artifact transitions and reads as every other backend
3. expose no Linear-specific semantics above the contract boundary

### 8.3 GitHub adapter

Responsibilities:

1. map the canonical Kata domain model onto GitHub Issues, sub-issues, Projects v2, pull requests, and associated artifact storage
2. support the same milestone/slice/task/artifact transitions and reads as every other backend
3. expose no GitHub-specific semantics above the contract boundary

Hard rule:

1. GitHub label-based workflow mode is removed entirely

## 9) Relationship to `apps/orchestrator`

The new platform replaces Kata Orchestrator as the planning/execution product, but it should reuse its structural ideas where they remain valuable.

Reuse candidates:

1. workflow decomposition (`discuss`, `plan`, `execute`, `verify`)
2. skill-driven product framing
3. command/workflow organization patterns
4. agent/skill packaging concepts where they align with the published Skills spec

Do not carry forward:

1. local markdown files as the canonical planning store
2. file-centric state assumptions as the primary workflow model
3. harness-specific product branching

The new system should feel like "Orchestrator's workflow intelligence, rebuilt around strict backend abstraction and a reusable CLI domain layer."

## 10) Packaging and Distribution

### 10.1 Canonical npm package

`@kata-sh/cli` is the canonical package.

It contains:

1. the typed Kata domain API implementation
2. backend adapters
3. setup/bootstrap flows
4. any command entrypoints needed for non-harness use, diagnostics, or direct local operation
5. the transport surfaces needed to consume the same contract from multiple environments

Recommended consumption modes:

1. local binary / shell command
2. embeddable Node library where direct in-process use is appropriate
3. stdio / JSON / tool-server surface for harnesses that prefer external tool invocation

Regardless of transport, the domain contract stays the same.

Primary onboarding path:

1. `npx @kata-sh/cli setup`
2. or first-run `kata setup`

The setup flow detects the target harness and installs/configures the appropriate integration, or falls back to an `npx`-driven path.

### 10.2 Canonical skill bundle

The skill bundle is the canonical workflow layer.

It should include:

1. init/setup skill
2. planning skills
3. execution and verification skills
4. PR/review skills
5. optional Symphony operator skills if needed

The bundle remains logically distinct from the CLI even when versioned together.

### 10.3 Harness packaging

The same skills and CLI are distributed through multiple packaging forms:

1. npm bootstrap via `@kata-sh/cli`
2. `skills.sh`
3. Codex plugin
4. Claude Code plugin
5. Cursor plugin
6. Pi integration package

These are delivery forms, not different products.

CI should generate the relevant distribution artifacts from the same canonical sources rather than maintaining separate hand-authored product variants.

### 10.4 Desktop distribution

Desktop ships as the integrated packaged solution with:

1. Pi coding-agent runtime in RPC mode
2. prepackaged Kata skills
3. Kata CLI
4. Symphony binaries

### 10.5 Release and publishing

CI owns:

1. release workflows
2. packaging
3. distribution artifact generation
4. publishing to the relevant destinations for npm, plugins, and other distribution channels

The runtime and setup flows should assume artifacts already exist and are simply being installed or activated.

## 11) Migration stance

Because planned artifacts remain unchanged, this design intentionally avoids compatibility scaffolding.

Rules:

1. no backward-compatibility layer is required
2. no fallback behavior is required
3. no dual-mode GitHub support is required
4. replace the old product architecture cleanly rather than carrying legacy branches forward

## 12) Risks and Tradeoffs

### 12.1 Primary risks

1. **Plugin flavor divergence**
   Different harnesses may require different config, manifest, hook, file layout, and publishing mechanics.

   Mitigation:
   Keep one canonical skill bundle and one canonical CLI, and generate/package harness-specific integration artifacts from CI.

2. **Skill/CLI contract drift**
   The skill layer could assume behavior that the CLI no longer guarantees.

   Mitigation:
   Version the contract explicitly, add compatibility metadata, and run CI contract tests between the canonical skills and the CLI.

3. **Adapter conformance failure**
   A backend adapter may fail to faithfully implement the canonical Kata contract.

   Mitigation:
   Treat the Kata domain API as normative and adapter conformance as a hard requirement. If quirks leak out, the adapter is incomplete.

4. **Desktop decoupling risk**
   Desktop currently contains assumptions tied to the existing Kata CLI wrapper.

   Mitigation:
   Make the boundary explicit: Pi RPC runtime concerns and Kata workflow/backend concerns are separate services and must be tested as such.

5. **Symphony interface drift**
   Symphony may rely on certain Kata-facing names or interface expectations.

   Mitigation:
   Keep the compatibility surface narrow and validate it in CI.

### 12.2 Tradeoffs

1. Skill-first architecture gives the strongest portability story and the cleanest product identity, but it requires discipline in skill design and contract testing.
2. A strict typed domain API adds upfront design cost, but it is what makes multiple backends and multiple harnesses sustainable.
3. Removing GitHub label mode simplifies the architecture and reduces fallback complexity, but intentionally narrows the supported GitHub workflow model.
4. Treating Desktop as the integrated first-class product increases packaging responsibility, but it creates the clearest end-user experience.

## 13) Compatibility Rules

The following rules are normative:

1. Skills are uniform because they target a published Skills spec.
2. Harness-specific variation is allowed only in plugin/config/install/hook/publishing mechanics.
3. Backend-specific variation is allowed only inside adapter internals.
4. Workflow semantics may not vary by harness.
5. Domain contract behavior may not vary by backend.

If any future feature requires:

1. backend-specific prompt branches
2. backend-specific skill variants
3. harness-specific workflow semantics

then the abstraction is incomplete and must be fixed in the CLI/adapters rather than papered over in the skill layer.

## 14) Validation Expectations

This design should be validated through:

1. CLI contract tests against the canonical domain API
2. adapter conformance tests ensuring GitHub Projects v2 and Linear produce identical contract behavior
3. skill-to-CLI compatibility tests
4. Desktop tests confirming direct Pi RPC runtime plus separate Kata CLI backend integration
5. Symphony compatibility checks for any changed Kata-facing names/tools
6. CI release tests for npm, plugin packaging, and integrated distribution outputs

## 15) Outcome

This design redefines Kata as:

1. a portable skill-first planning/execution product
2. backed by a strict typed Node domain API
3. packaged into multiple harness/plugin flavors
4. delivered most completely through Desktop
5. executed autonomously through Symphony without changing Symphony's core backend-polled model

The result is not "Kata as a custom Pi wrapper." It is "Kata as a portable workflow platform with a canonical skills layer, a canonical domain/backend CLI, and a strict contract that every backend and harness integration must honor."
