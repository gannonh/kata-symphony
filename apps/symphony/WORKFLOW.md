---
tracker:
  # Choose `github` or `linear`.
  kind: github

  # GitHub tracker settings. Replace these for your repository/project.
  repo_owner: your-github-owner
  repo_name: your-repo-name
  github_project_owner_type: user
  github_project_number: 1

  # Linear tracker settings, used when `kind: linear`.
  # workspace_slug: your-linear-workspace
  # project_slug: your-linear-project

  active_states:
    - Todo
    - In Progress
    - Agent Review
    - Merging
    - Rework
  terminal_states:
    - Done
  exclude_labels:
    - kata:task
polling:
  interval_ms: 30000
workspace:
  # Relative to process cwd (run Symphony from the repository root).
  # Prompts/hooks are relative to this WORKFLOW.md directory instead.
  root: .symphony/workspaces
  repo: .
  git_strategy: worktree
  isolation: local
  cleanup_on_done: false
  branch_prefix: symphony
  clone_branch: main
  base_branch: main
hooks:
  timeout_ms: 1200000
agent:
  name: pi
  command: pi --mode rpc
  no_session: false
  max_concurrent_agents: 4
  max_turns: 20
  # Set the default model for your agent harness.
  # model: provider/model-name
  stall_timeout_ms: 900000
prompts:
  system: prompts/system.md
  repo: prompts/repo.md
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
# Durable factory-run storage (A1 triage). Disabled triage uses no SQLite lock.
# storage:
#   # Optional override. Default is a namespaced path under the platform data dir.
#   # path: $SYMPHONY_STATE_PATH
#   busy_timeout_ms: 5000
# GitHub Issues triage (A1 preview). Keep disabled until doctor checks pass.
# triage:
#   enabled: false
#   mode: preview # preview | automatic
#   intake_label: needs-triage
#   prompt: prompts/triage.md
#   # model: provider/model-name  # Pi only; rejected for Codex
#   turn_timeout_ms: 900000
#   max_attempts: 3
#   max_intake_pages: 100
#   routes:
#     implement:
#       label: ready-for-agent
#       state: Todo
#     spec:
#       label: ready-to-spec
#     needs_information:
#       label: needs-info
#     park:
#       label: wait-to-implement
#     human_owned:
#       label: ready-for-human
# Reviewed, versioned GitHub specification stage (A2).
# spec:
#   enabled: false
#   intake_label: ready-to-spec
#   prompts:
#     draft: prompts/spec-draft.md
#     review: prompts/spec-review.md
#     revise: prompts/spec-revise.md
#   # model: provider/model-name         # Pi only
#   # review_model: provider/model-name  # defaults to resolved draft model
#   turn_timeout_ms: 1800000
#   max_intake_pages: 100
#   max_review_cycles: 3
#   max_attempts: 3
#   max_revision_requests: 3
#   labels:
#     approved: spec-approved
#     revise: spec-revise
#   approval_route:
#     label: ready-for-agent
#     state: Todo
# Spec-driven implementation stage (A3). Requires a terminal A2 approval pin.
# Preview mode posts an owned issue comment only; automatic mode (PR2) opens a
# draft PR and moves the issue to completion_route.state.
# implementation:
#   enabled: false
#   mode: preview
#   prompt: prompts/implementation.md
#   repair_prompt: prompts/implementation-repair.md
#   # model: provider/model-name         # Pi only
#   max_turns: 20
#   invocation_timeout_ms: 3600000
#   max_attempts: 3
#   max_validation_cycles: 3
#   max_bundle_bytes: 104857600
#   spec_file: specs/{issue_identifier}/APPROVED-v{version}.md
#   validation:
#     - name: affected-validation
#       command: pnpm run validate:affected
#       timeout_ms: 1800000
#   # completion_route:
#   #   state: Agent Review
# notifications:
#   slack:
#     webhook_url: $SLACK_WEBHOOK_URL
#     events:
#       - all
---
