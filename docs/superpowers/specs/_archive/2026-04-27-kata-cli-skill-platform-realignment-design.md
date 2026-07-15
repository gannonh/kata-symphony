---
type: Spec
title: Kata Cli Skill Platform Realignment Design
description: Archived spec document: Kata Cli Skill Platform Realignment Design.
tags: [archive, cli]
timestamp: 2026-07-15T20:00:00Z
---

# Kata CLI Skill Platform Realignment Design

**Date:** 2026-04-27  
**Status:** Draft for approval  
**Scope:** Full multi-phase architecture with Phase A as immediate implementation target

## 1. Why This Reset Exists

The prior migration work produced partial structural progress but did not produce a reliable, verifiable product surface that matches the intended architecture end-to-end.

This design resets execution around one rule:

**We ship only what is actually integrated and verifiable against real backend behavior.**

## 2. Design Goals

1. Keep Kata as a skills-first system portable across harnesses.
2. Make `@kata-sh/cli` the sole owner of durable workflow IO and backend side effects.
3. Use GitHub Projects v2 as the concrete backend path for Phase A validation.
4. Preserve Desktop runtime model (Pi RPC direct).
5. Remove workflow branching complexity by integrating alignment/discussion into primary workflows.
6. Deliver in explicit phases, with only Phase A planned for implementation now.

## 3. Locked Architectural Decisions

1. **Primary Kata behavior lives in Skills.**
2. **All durable IO lives in CLI typed APIs.**
3. **Backend behavior is adapter-internal and invisible above contract boundary.**
4. **No standalone `discuss-*` commands.** Discussion/alignment is integrated into workflow skills.
5. **`new-project` does not create milestones.** It concludes by routing the user to `new-milestone`.
6. **Wrapper scripts must stay thin.** Any `./scripts/*.mjs` wrappers delegate to CLI (installing if missing).
7. **Phase A proof requires real backend execution.** No mocks, no JSON-only proof.

## 4. End-to-End Workflow Model

### 4.1 Canonical Workflow Family

Primary workflow skills:

1. `kata-setup`
2. `kata-new-project`
3. `kata-new-milestone`
4. `kata-plan-phase`
5. `kata-execute-phase`
6. `kata-verify-work`
7. `kata-complete-milestone`
8. `kata-progress`
9. `kata-health`

Standalone `kata-discuss-*` skills/commands are removed.

### 4.2 Integrated Alignment Pattern

Each primary workflow begins with integrated alignment depth:

1. `fast`
2. `guided` (default)
3. `deep`

Implementation pattern:

1. Shared alignment template (common structure/guardrails)
2. Workflow-specific overlays (questions and decisions relevant to that workflow)

### 4.3 Phase A Acceptance Chain

Phase A required chain on a real GitHub Projects v2 project:

1. `kata-setup`
2. `kata-new-project`
3. `kata-new-milestone`
4. `kata-plan-phase`
5. `kata-execute-phase`
6. `kata-verify-work`
7. `kata-complete-milestone`
8. `kata-new-milestone`
9. `kata-plan-phase`

## 5. Domain Primitives and Contract Ownership

## 5.1 Core Primitives

1. `Project`
2. `Milestone`
3. `Slice` (vertical delivery unit)
4. `Task`
5. `Artifact`

## 5.2 Artifact Types for Phase A

1. `project-brief`
2. `requirements`
3. `roadmap`
4. `phase-context`
5. `research`
6. `plan`
7. `summary`
8. `verification`
9. `uat`

## 5.3 Ownership Rules

1. Skills orchestrate behavior and sequencing.
2. CLI performs all durable reads/writes/transitions.
3. Skills do not implement backend-specific storage behavior.
4. Desktop and Symphony consume contract behavior, not adapter quirks.

## 5.4 Backend Rules

1. Contract semantics must be identical above adapter internals.
2. GitHub Projects v2 is the real backend path for Phase A.
3. Local fallback stores cannot serve as production-path proof.

## 6. Skills Progressive Disclosure Model

Each workflow skill ships with:

1. `SKILL.md` (compact orchestration/trigger/guardrails)
2. `references/setup.md`
3. `references/alignment.md`
4. `references/workflow.md`
5. `references/runtime-contract.md`

Guidelines:

1. Keep `SKILL.md` concise.
2. Put deep instructions in references.
3. Keep references self-contained and portable.
4. Avoid harness-local path assumptions in shipped skill content.

## 7. Script Wrapper Rule

Helper scripts are allowed, but only as thin wrappers.

Rules:

1. Script checks for CLI availability.
2. Script installs CLI if missing (or instructs deterministic install path).
3. Script delegates business logic to CLI typed operations.
4. Script contains no duplicated workflow/business logic.

## 8. Desktop and Runtime Boundaries

1. Desktop uses Pi directly in RPC mode.
2. Desktop packages Pi runtime, Kata skills, Kata CLI, and Symphony binaries.
3. Desktop workflow reads/writes route through the CLI contract.
4. Desktop does not become a second workflow backend implementation.

## 9. Multi-Phase Delivery Roadmap

### Phase A (implementation plan now)

Pi + GitHub Projects v2 end-to-end correctness with integrated workflow model and real backend IO.

### Phase B

Backend parity/hardening (including stricter adapter conformance and robustness).

### Phase C

Harness packaging expansion (Codex/Claude/Cursor/skills.sh distribution surfaces).

### Phase D

Desktop/runtime maturity and packaging ergonomics hardening.

### Phase E

Symphony validation against new skill/CLI surface.

### Phase F

Dedicated e2e CLI/Skills/backend IO test framework and eval system.

## 10. Phase A Exit Criteria

Phase A is done only if all are true:

1. Required acceptance chain completes in real Pi + GitHub Projects v2.
2. Artifacts are created/read/updated through CLI contract operations on real backend state.
3. Milestone rollover path is proven (`complete-milestone -> new-milestone -> plan-phase`).
4. Standalone discuss commands are removed from shipped workflow surface.
5. Integrated alignment pattern works inside primary workflows.
6. No mocked backend path is used as acceptance evidence.

## 11. Risks and Controls

1. **Legacy workflow content leakage**
Control: Rewrite workflow references for primary Phase A skills before acceptance.

2. **Contract drift between skills and CLI**
Control: Keep runtime-contract references aligned with typed operations and enforce compatibility checks.

3. **Wrapper script behavior creep**
Control: Keep wrapper scripts tiny and delegate all logic to CLI.

4. **False-positive validation**
Control: Require real backend state evidence in acceptance runbook.

## 12. What This Design Intentionally Defers

1. Full multi-harness execution parity beyond Pi in Phase A.
2. Symphony conformance completion (Phase E).
3. Dedicated e2e/eval framework buildout (Phase F).

Those are committed as explicit future phases, not hidden work.

## 13. Immediate Next Step

After approval of this design:

1. Write a **Phase A-only implementation plan**.
2. Execute only against Phase A scope and exit criteria.
