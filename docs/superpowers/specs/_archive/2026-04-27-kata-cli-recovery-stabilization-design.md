---
type: Spec
title: Kata Cli Recovery Stabilization Design
description: Archived spec document: Kata Cli Recovery Stabilization Design.
tags: [archive, cli]
timestamp: 2026-07-15T20:00:00Z
---

# Kata CLI Recovery and Stabilization Design

Date: `2026-04-27`

## Problem Statement

The current migration branch contains meaningful foundations (typed domain API, adapters, skill generation, desktop bundling), but it is not yet a reliable product path. The largest issue is trust drift between:

1. legacy orchestrator workflow documents
2. emitted harness-facing skills
3. executable runtime behavior in real harnesses/backends

Without a single validated golden path, implementation effort feels ambiguous and expensive to verify.

## Recovery Objective

Re-baseline the migration around one production-grade golden path first:

- Harness: `Pi`
- Backend: `GitHub Projects v2` only
- Surface: `Kata Skills + standalone @kata-sh/cli runtime`
- Desktop: direct `Pi` RPC integration (no custom Kata CLI RPC wrapper)

All other harness packaging remains secondary until this path is green in CI and manually reproducible.

## Non-Negotiable Constraints

1. Backend contract is fully abstracted and typed; skill/runtime code cannot contain backend-specific branching.
2. GitHub label-based mode is removed with no fallback.
3. Skills remain uniform by published Skills spec; harness differences are adapter/plugin packaging only.
4. Symphony execution architecture remains unchanged, except tool/skill name alignment if needed.
5. CI owns build, verification, and distribution workflows.

## Architecture Decisions

### 1) Truth Model

There are three distinct layers and they must not be conflated:

- `apps/orchestrator/kata/workflows/*`: legacy-heavy migration input corpus
- `apps/orchestrator/dist/skills/*`: actual harness-visible skill surface
- `apps/cli` runtime + domain API + adapters: actual executable backend behavior

A capability is considered migrated only when all are true:

1. source intent exists (workflow or equivalent canonical spec)
2. skill surface exists (or intentional consolidation is documented)
3. runtime executes against live backend through typed contract

### 2) Golden Path First

The first shippable target is:

`Pi -> Kata Skill -> @kata-sh/cli -> GitHub Projects v2`

No cross-harness parity claim is allowed before this path passes.

### 3) Backend Abstraction Boundary

`@kata-sh/cli` domain API is the only backend contract consumed by:

- skills
- desktop backend client
- optional transport surfaces (`kata json`, future RPC wrappers)

Backend adapters (GitHub/Linear) are replaceable implementations of this same contract.

### 4) Desktop Role

Desktop is the integrated product distribution:

- bundles Pi runtime, Kata CLI runtime, Kata skills, Symphony binary
- launches Pi directly in RPC mode
- reads planning/workflow state through CLI/domain boundary

## Scope (Stabilization Sprint)

### In Scope

1. Capability matrix and migration truth docs.
2. Live GitHub adapter wiring through standalone runtime.
3. Real `setup --pi` install flow (skills + config/hooks + verification).
4. Core skill set end-to-end on golden path.
5. CI release gates for golden path regression protection.

### Out of Scope (Until Golden Path Is Green)

1. Advanced Claude/Codex/Cursor installer automation parity.
2. Expanding skill catalog beyond core operational workflow.
3. Symphony behavioral redesign.
4. Any fallback compatibility mode for GitHub label workflow.

## Deliverables and Exit Gates

### Gate A: Migration Truth and Coverage Control

- Capability matrix checked in (workflow -> skill -> runtime op -> test evidence -> status).
- Every workflow not emitted as a skill is explicitly labeled:
  - `consolidated`
  - `internal-only`
  - `pending`

Exit condition: no “unknown” rows.

### Gate B: Runtime Execution Completeness

- Standalone runtime routes real GitHub clients (no stubs) for required operations.
- Domain API tests pass with strict contract assertions.
- Label mode rejected consistently across config readers/docs/types.

Exit condition: live GitHub smoke tests pass.

### Gate C: Pi Installation and Skill Discovery

- `npx @kata-sh/cli setup --pi` performs real install and verification.
- Installed skills are discoverable by Pi in a clean environment.
- `kata doctor` reports actionable pass/fail signals.

Exit condition: clean-machine local runbook passes.

### Gate D: Desktop Integrated Validation

- Desktop uses direct Pi RPC path.
- Desktop runtime bundle includes Pi + skills + CLI + Symphony.
- Manual test runbook proves desktop path for same GitHub project.

Exit condition: runbook executed successfully and captured in docs.

### Gate E: CI and Release Guardrails

- CI validates:
  - skill bundle coverage invariants
  - golden path smoke tests
  - distribution artifact completeness
- Release workflows publish only validated artifact sets.

Exit condition: CI fails on any golden-path breakage.

## Risks and Mitigations

### Risk: False Confidence from Artifact-Only CI

Mitigation: introduce behavior-level smoke tests (Pi + skill invocation + backend operation), not only file existence.

### Risk: Workflow Corpus Drift

Mitigation: enforce matrix ownership and fail CI if manifest/skills/workflow mappings become inconsistent.

### Risk: Backend-Specific Logic Leaks

Mitigation: constrain skill/runtime logic to domain API methods only; adapter-specific branching stays inside adapter packages.

### Risk: Multi-Harness Expansion Too Early

Mitigation: sequence work so other harness adapters remain packaging wrappers until golden path is stable.

## Validation Strategy

### Automated

1. Adapter/domain contract tests in `apps/cli`.
2. Skill bundle generation/coverage tests in `apps/orchestrator`.
3. Distribution build checks in `scripts/ci/build-kata-distributions.sh`.
4. Golden-path smoke test lane for Pi + GitHub Projects v2.

### Manual

1. Clean-environment Pi setup via npm/npx.
2. Skill discovery and invocation check.
3. Real GitHub board read/write flow through runtime.
4. Desktop integrated run through same project.

## Success Criteria

This recovery is complete when all are true:

1. A new developer can run one documented setup flow and execute core Kata skills in Pi against GitHub Projects v2.
2. Desktop executes the same backend path with direct Pi RPC and bundled runtime artifacts.
3. CI prevents regression of that path.
4. Remaining work (Linear parity, additional harness installers, expanded skills) is clearly staged and does not block current reliability.
