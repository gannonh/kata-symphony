# Building Symphony With Harness Engineering

Last reviewed: 2026-03-05

## Goal

Use harness engineering while building Symphony, and ship Symphony with the same discipline.

## Track 1: Build-time harness (repo now)

1. Agent map and docs structure in place
2. Explicit quality/reliability/security baseline docs
3. Scripted repository contract checks
4. CI workflow that runs harness checks on every PR

## Track 2: Runtime harness (product behavior)

1. Enforce workflow/config validation before dispatch
2. Enforce isolation and safety in workspace execution
3. Enforce retry/reconciliation correctness and recovery behavior
4. Expose operator monitoring surfaces (TUI MVP, then web dashboard)

## Dogfooding Loop

1. Build Symphony in this repo using these harness docs and checks.
2. Use Symphony workflows to execute Symphony backlog work.
3. Feed incidents and friction back into:
   - `AGENTS.md`
   - runtime defaults in `WORKFLOW.md`
   - test matrix and operational docs

