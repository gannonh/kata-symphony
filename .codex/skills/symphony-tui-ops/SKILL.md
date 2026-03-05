---
name: symphony-tui-ops
description: Operate and validate Symphony TUI monitoring workflows. Use when building, testing, or running the terminal monitoring surface for orchestrator sessions, retries, health, token usage, and event tails.
---

# Symphony TUI Ops

## Overview

Guide TUI implementation and operations for Symphony monitoring.

## Workflow

1. Verify runtime data contract (snapshot/event feed).
2. Validate TUI behaviors:
   - panel rendering
   - keyboard navigation
   - filtering
   - refresh/reconnect handling
3. Run scenario checks against a live Symphony instance:
   - active runs
   - queued retries
   - error/stall conditions
4. Record operator-facing defects and follow-up work.

## Acceptance Checklist

- TUI remains usable across common terminal sizes.
- Core monitoring data stays current with low latency.
- Empty/loading/error states are clear and actionable.

## Output

Provide a TUI ops verification report with evidence and unresolved gaps.
