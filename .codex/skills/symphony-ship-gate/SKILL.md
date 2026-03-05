---
name: symphony-ship-gate
description: Evaluate Symphony release readiness and produce explicit go/no-go decisions with blockers, mitigations, and residual-risk posture.
---

# Symphony Ship Gate

## Purpose

Perform release checkpoint decisions with explicit criteria and evidence.

## Use This For

- Milestone release readiness checks
- Go/no-go decisions for promotion
- Final blocker/risk review

## Do Not Use This For

- Ticket lifecycle/state transitions
- Generic feature implementation workflow
- Replacing test execution with judgment-only assessments

## Workflow

1. Confirm release candidate scope/commit.
2. Review evidence:
   - conformance status
   - `make check` and required tests
   - open blockers and known risks
   - security/reliability posture
3. Classify remaining issues:
   - must-fix
   - acceptable with mitigation
   - defer
4. Issue explicit decision: `Go` or `No-Go`.
5. List mitigation and monitoring actions.

## Output

- Decision (`Go`/`No-Go`)
- Blocking gaps
- Required mitigations
- Post-release monitoring focus
