---
type: ADR
title: Triage process recovery identity
status: Accepted
description: Authorize interrupted-attempt process-group recovery with PID, process group, and OS start token while retaining executable identity as diagnostics.
tags: [symphony, triage, adr, recovery, process]
timestamp: 2026-07-25T22:35:00Z
---

# ADR-0002: Triage process recovery identity

## Status

Accepted (2026-07-25) during A1 PR3 remediation.

## Context

Symphony records a triage child immediately after spawn so a replacement
Symphony process can terminate an orphaned attempt. The initial implementation
required PID, process group, OS start token, and executable to remain equal.

That executable comparison rejected a legitimate launcher transition observed
during A1 PR3 Verify: a recorded shell used `exec` to become the worker. Its
PID, process group, and Linux start token remained stable, but `/proc/<pid>/exe`
changed. Recovery removed the workspace and retried while leaving the worker
alive. The same race made a test nondeterministic when identity capture occurred
before the child's initial `exec`.

## Decision

1. **Stable authorization tuple** — Recovery authorizes a signal using the
   process leader PID, process-group ID, and OS process start token.
2. **Executable is diagnostic** — The executable observed at spawn remains in
   SQLite and the HTTP stage-run record for compatibility and diagnosis. An
   executable difference is logged as drift and does not deny signaling.
3. **Fail closed** — Invalid identifiers, Symphony's own group, a missing start
   token, an absent process, an unreadable live token, a group mismatch, or a
   start-token mismatch deny signaling with a structured reason.
4. **Recheck before signal** — Bounded termination repeats the stable-identity
   assessment immediately before sending `SIGTERM`.
5. **Verified bounded termination** — Recovery waits five seconds after
   `SIGTERM`, sends `SIGKILL` when a running group member remains, and confirms
   that no running/non-zombie group member remains before reporting success.
6. **Retain unresolved recovery state** — Symphony clears durable process and
   workspace fields only after termination or confirmed process absence.
   Signal denial, identity uncertainty, and bounded `StillRunning` outcomes keep
   the record for later recovery polls.
7. **Isolate every supported harness** — On Unix, Pi and Codex triage children
   start as process-group leaders so recovery never targets Symphony's inherited
   group.
8. **Authorize recursive cleanup from configuration and identity** — Recovery
   removes only the exact `triage-<stage_run_id>` directory directly under the
   configured `workspace.root`.

## Consequences

- Launchers may safely replace themselves through `exec` without becoming
  unrecoverable.
- PID reuse remains guarded by the OS start token and exact process-group match.
- No SQLite migration or breaking HTTP schema change is needed.
- Linux uses `/proc/<pid>/stat` start ticks. Other supported systems use the
  coarser `ps -o lstart=` value and retain that platform limitation.
- A small time-of-check/time-of-signal race remains because POSIX group
  signaling is not an atomic identity-and-signal operation. Rechecking directly
  before the signal minimizes but cannot eliminate that race.
- A zombie leader may remain until its parent or init reaps it; zombies are not
  treated as running workers.
- A narrow child-spawn-to-SQLite-persistence window remains. Eliminating it
  requires runner-side durable identity recording and is outside this decision's
  current implementation.

## Links

- Spec: [A1 GitHub Issue Triage](/specs/archive/2026-07-16-a1-github-issue-triage-design.md)
- Rejected Verify: [A1 PR3 Verify report](/specs/archive/2026-07-25-a1-pr3-verify-report.md)
- Re-verify and review closeout: [A1 PR3 re-verify report](/specs/archive/2026-07-25-a1-pr3-reverify-report.md)
- Implementation: `apps/symphony/src/triage/process_identity.rs`
