---
name: symphony-runtime-hardening
description: Harden Symphony runtime safety posture and policy defaults. Use when changing approval/sandbox behavior, hook execution policy, workspace isolation, secret handling, logging redaction, or operational safety controls.
---

# Symphony Runtime Hardening

## Purpose

Apply runtime safety changes without regressing orchestration behavior.

## Use This For

- Approval/sandbox default changes
- Hook isolation, timeout, and failure semantics
- Secret handling and redaction policies
- Workspace containment/safety invariants

## Do Not Use This For

- Ticket lifecycle/state transitions
- Generic implementation planning/execution workflow
- PR/merge automation

## Workflow

1. Identify safety surface and blast radius.
2. Validate against `SECURITY.md`, `RELIABILITY.md`, and relevant `SPEC.md` sections.
3. Implement minimal hardening delta.
4. Add/adjust tests for changed invariants.
5. Run `make check` and targeted tests.
6. Record residual risks and deferred controls.

## Output

- Hardening delta summary
- Compatibility impact
- Verification evidence
- Deferred risk items
