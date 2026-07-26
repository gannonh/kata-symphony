---
tracker:
  kind: github
  repo_owner: gannonh
  repo_name: kata-symphony
  github_project_owner_type: user
  github_project_number: 17
  active_states:
    - Todo
    - In Progress
    - Agent Review
    - Merging
    - Rework
  exclude_labels:
    - kata:task
  terminal_states:
    - Done
polling:
  interval_ms: 3000
workspace:
  root: /Volumes/EVO/symphony-workspaces
  repo: /Volumes/EVO/dev/kata-symphony
  git_strategy: worktree
  isolation: local
  cleanup_on_done: true
  branch_prefix: kata-symphony
  clone_branch: main
  base_branch: main
hooks:
  timeout_ms: 1200000
  # Run after workspace directory is created (after git bootstrap).
  after_create: ../scripts/bootstrap-symphony-worktree.sh /Volumes/EVO/dev/kata-symphony
agent:
  name: pi
  command: pi --mode rpc
  no_session: false
  max_concurrent_agents: 8
  max_turns: 20
  model: openai-codex/gpt-5.5
  model_by_state:
    Agent Review: openai-codex/gpt-5.3-codex
    Merging: openai-codex/gpt-5.3-codex
  stall_timeout_ms: 900000
prompts:
  system: prompts/system.md # injected every turn
  repo: prompts/repo.md # injected every turn
  by_state:
    Todo: prompts/in-progress.md
    In Progress: prompts/in-progress.md
    Agent Review: prompts/agent-review.md
    Merging: prompts/merging.md
    Rework: prompts/rework.md
  default: prompts/in-progress.md
supervisor:
  enabled: true
  steer_cooldown_ms: 120000
server:
  port: 8080
  host: 127.0.0.1
notifications:
  slack:
    webhook_url: $SLACK_WEBHOOK_URL
    events:
      - todo
      - in_progress
      - agent_review
      - human_review
      - merging
      - rework
      - done
      - stalled
      - failed
---
