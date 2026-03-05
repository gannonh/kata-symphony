---
name: symphony-dogfood
description: Run and evaluate the “Symphony builds Symphony” dogfood loop. Use when selecting safe internal tickets for autonomous execution, collecting runtime outcomes, and feeding findings back into workflow defaults and harness docs.
---

# Symphony Dogfood

## Overview

Operate controlled dogfood cycles where Symphony executes Symphony backlog work.

## Workflow

1. Select safe candidate tickets:
   - clear acceptance criteria
   - low blast radius
   - no sensitive external side effects
2. Run execution under current `WORKFLOW.md` policy.
3. Collect metrics:
   - throughput
   - retry behavior
   - failure classes
   - human intervention frequency
4. Summarize outcomes and open follow-up issues.
5. Update harness docs/policies based on findings.

## Guardrails

- Start with read-only or low-risk tickets.
- Require explicit rollback path for risky changes.
- Stop cycle if repeated failure pattern appears.

## Output

Publish a dogfood report with measurable outcomes and recommended adjustments.
