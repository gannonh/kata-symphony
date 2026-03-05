---
name: symphony-ship-gate
description: Evaluate Symphony release readiness and produce a go/no-go decision. Use when preparing milestone closure, pre-release checks, or production promotion decisions.
---

# Symphony Ship Gate

## Overview

Run final release readiness checks with explicit decision criteria.

## Workflow

1. Confirm target scope and release candidate commit.
2. Validate readiness inputs:
   - conformance status (Sections 17/18)
   - harness checks (`make check`)
   - open blocker review
   - reliability/security posture
3. Classify remaining risks:
   - must-fix
   - acceptable with mitigation
   - defer
4. Produce go/no-go recommendation.

## Decision Rules

- `No-Go` if required conformance requirements are unmet.
- `No-Go` if critical safety risks lack mitigation.
- `Go` only with explicit residual-risk acknowledgment.

## Output

Provide:
- decision (`Go` or `No-Go`)
- blocking gaps
- mitigation actions
- post-release monitoring focus
