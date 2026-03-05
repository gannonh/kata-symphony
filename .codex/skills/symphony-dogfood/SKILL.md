---
name: symphony-dogfood
description: Run controlled “Symphony builds Symphony” cycles and produce measurable findings to improve runtime defaults, docs, and guardrails.
---

# Symphony Dogfood

## Purpose

Run safe internal dogfood loops and convert outcomes into concrete improvements.

## Use This For

- Selecting low-risk tickets for autonomous execution
- Tracking throughput, retries, failure classes, intervention rate
- Producing improvement recommendations from real runs

## Do Not Use This For

- Ticket lifecycle/state transitions
- Replacing normal implementation workflow for all tickets
- High-risk changes without rollback paths

## Workflow

1. Select candidate tickets with clear acceptance criteria and low blast radius.
2. Define stop conditions and rollback boundaries.
3. Run dogfood cycle and collect runtime metrics/evidence.
4. Categorize failures by class and recurrence.
5. Open follow-up tickets/docs updates for recurring patterns.
6. Summarize go-forward defaults/guardrails.

## Output

- Dogfood report with quantitative outcomes
- Failure taxonomy and intervention data
- Concrete follow-up actions
