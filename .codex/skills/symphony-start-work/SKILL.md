---
name: symphony-start-work
description: Start implementation of a Symphony issue with the correct repository context, branch setup, and TDD plan. Use when beginning work on a Linear ticket for this repo, especially when the task touches SPEC.md, WORKFLOW.md, orchestration behavior, or harness policy.
---

# Symphony Start Work

## Overview

Prepare a Symphony ticket for safe execution before editing code.

## Workflow

1. Read ticket scope and acceptance criteria.
2. Read relevant contract files:
   - `SPEC.md`
   - `WORKFLOW.md`
   - `AGENTS.md`
   - `ARCHITECTURE.md`
   - `PLANS.md`
3. Map ticket to SPEC sections it must satisfy.
4. Identify dependency and risk flags:
   - blocked-by tickets
   - runtime safety implications
   - docs impacted
5. Read the Linear ticket `gitBranchName` value and create/switch to that branch before coding.
6. Define a TDD plan (tests first): red -> green -> refactor.
7. Define verification steps, including repository gates.
8. Move into implementation only after boundaries, branch, and TDD plan are clear.

## Output

Produce a short kickoff summary containing:
- scope in/out
- SPEC sections touched
- branch name used
- first failing test(s) to implement
- verification plan
- docs expected to change
