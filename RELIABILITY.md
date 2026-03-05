# Symphony Reliability Notes

Last reviewed: 2026-03-05

## Reliability Objectives

1. Preserve orchestrator correctness under transient failures.
2. Recover from process restarts without a durable DB.
3. Keep reconciliation active even when dispatch validation fails.

## Required Behaviors (from `SPEC.md`)

- Reconciliation before dispatch on every tick
- Exponential retry with cap for abnormal exits
- Continuation retry after normal exits
- Stall detection with configurable timeout
- Startup terminal workspace cleanup

## Operational Signals

- Running issue count
- Retry queue depth
- Retry age percentiles
- Worker abnormal exit rate
- Reconciliation failure count

## Failure Classes

- Tracker transport/auth/query failures
- Workspace lifecycle/hook failures
- Agent protocol/timeouts/stalls
- Configuration reload/validation failures

