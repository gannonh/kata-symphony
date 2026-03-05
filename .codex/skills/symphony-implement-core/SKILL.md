---
name: symphony-implement-core
description: Implement Symphony core issues with harness discipline. Use when writing or modifying code for orchestrator, tracker, workspace, agent runner, retries, reconciliation, workflow loading, or observability features.
---

# Symphony Implement Core

## Overview

Execute a Symphony issue with strict harness and documentation gates.

## Workflow

1. Confirm scope from the ticket and `SPEC.md`.
2. Write or update failing tests first (TDD red phase).
3. Implement minimum viable change that satisfies acceptance criteria (TDD green phase).
4. Refactor while keeping tests passing (TDD refactor phase).
5. Update docs affected by behavior changes.
6. Run repository gates:
   - `make lint`
   - `make check`
7. Fix failures before claiming completion.
8. Capture verification evidence tied to ticket acceptance criteria.

## Implementation Rules

- Keep changes small and reviewable.
- Preserve safety invariants for workspace containment and runtime policy.
- Avoid speculative refactors outside ticket scope.
- Do not mark work complete without passing checks.

## Output

Provide:
- changed files
- behavior summary
- verification commands and outcomes
- residual risk/follow-up notes
