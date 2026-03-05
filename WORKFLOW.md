---
tracker:
  kind: linear
  endpoint: https://api.linear.app/graphql
  api_key: $LINEAR_API_KEY
  project_slug: symphony-service-v1-spec-execution
  active_states:
    - Todo
    - In Progress
  terminal_states:
    - Done
    - Duplicate
    - Canceled
    - Cancelled
polling:
  interval_ms: 30000
workspace:
  root: $SYMPHONY_WORKSPACE_ROOT
hooks:
  timeout_ms: 60000
agent:
  max_concurrent_agents: 5
  max_turns: 20
  max_retry_backoff_ms: 300000
codex:
  command: codex app-server
  turn_timeout_ms: 3600000
  read_timeout_ms: 5000
  stall_timeout_ms: 300000
---

You are working on a Symphony issue from Linear.

Issue:
- Identifier: `{{ issue.identifier }}`
- Title: `{{ issue.title }}`
- State: `{{ issue.state }}`
- Priority: `{{ issue.priority }}`
- Attempt: `{{ attempt }}`

Execution rules:
1. Follow `SPEC.md` as the product contract.
2. Keep changes scoped to the current issue acceptance criteria.
3. Run available tests/checks before handoff.
4. Update docs if behavior or operational posture changes.
5. Prefer safe, incremental changes with clear rollback paths.

Completion handoff:
1. Summarize behavior changes and verification evidence.
2. Call out residual risk and follow-up work.
3. Move issue to the workflow-defined handoff state when ready.

