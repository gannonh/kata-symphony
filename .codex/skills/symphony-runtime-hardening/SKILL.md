---
name: symphony-runtime-hardening
description: Harden Symphony runtime safety posture and policy defaults. Use when changing approval/sandbox behavior, hook execution policy, workspace isolation, secret handling, logging redaction, or operational safety controls.
---

# Symphony Runtime Hardening

## Overview

Apply and validate runtime safety controls without breaking orchestration correctness.

## Workflow

1. Identify safety surface being changed:
   - codex approval/sandbox policy
   - workspace hooks and containment
   - secrets/logging behavior
   - operator intervention paths
2. Validate against:
   - `SECURITY.md`
   - `RELIABILITY.md`
   - `SPEC.md` Sections 10.5, 14, and 15
3. Implement hardening change with clear defaults.
4. Add/adjust tests and run `make check`.
5. Document tradeoffs and deferred controls.

## Output

Provide:
- hardening delta
- compatibility impact
- test evidence
- deferred risk register updates
