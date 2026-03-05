---
name: symphony-tui-ops
description: Build and validate Symphony operator TUI monitoring behavior for sessions, retries, health, token usage, and event tails.
---

# Symphony TUI Ops

## Purpose

Guide TUI-specific implementation and validation for operator monitoring.

## Use This For

- TUI panel behavior and layout validation
- Keyboard navigation and filtering flows
- Refresh/reconnect/error-state behavior
- Live-run monitoring checks against Symphony runtime

## Do Not Use This For

- Ticket lifecycle/state transitions
- Generic project planning workflow
- Non-TUI backend changes unless directly required by TUI behavior

## Workflow

1. Confirm snapshot/event data contract used by the TUI.
2. Validate panel rendering for loading/empty/error states.
3. Validate keyboard interactions and filter performance.
4. Run live scenarios (active runs, retries, failures, stalls).
5. Capture operator-facing defects and follow-ups.
6. Run `make check` plus TUI-specific tests.

## Output

- TUI behavior verification report
- Defect list with repro notes
- Evidence for pass/fail criteria
