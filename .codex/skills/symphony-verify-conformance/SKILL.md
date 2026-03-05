---
name: symphony-verify-conformance
description: Verify Symphony changes against SPEC.md conformance requirements. Use when validating tickets before review/merge, especially for Sections 17 and 18 test and definition-of-done coverage.
---

# Symphony Verify Conformance

## Overview

Map implementation evidence to SPEC requirements and highlight gaps.

## Workflow

1. Identify relevant requirement set in `SPEC.md`:
   - Section 17.x validation matrix
   - Section 18.x definition-of-done checklist
2. Build a requirement-to-evidence mapping.
3. Run and record required checks/tests.
4. Mark each requirement as:
   - pass
   - fail
   - not-covered
5. Open follow-up tasks for uncovered or failing requirements.

## Evidence Format

Use a compact table:
- requirement
- status
- evidence (test/log/file)
- notes

## Output

Produce a conformance summary with explicit blockers to merge or release.
