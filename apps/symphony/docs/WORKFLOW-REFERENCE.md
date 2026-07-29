---
# ═══════════════════════════════════════════════════════════════════════════════
# Symphony WORKFLOW.md — Orchestrator Configuration + Agent Prompt Template
# ═══════════════════════════════════════════════════════════════════════════════
#
# This file serves two purposes:
#   1. YAML front-matter: parsed by Symphony as runtime configuration
#   2. Markdown body: rendered as the Liquid prompt template for each agent session
#
# `symphony init` writes this file to `.symphony/WORKFLOW.md` with prompt files
# under `.symphony/prompts/`. When no workflow path is passed, Symphony tries
# `.symphony/WORKFLOW.md` first, then `WORKFLOW.md` for compatibility.
#
# Symphony watches this file for changes and applies config updates without
# requiring a process restart.
#
# Environment variable indirection: any string value starting with `$` followed
# by a bare identifier (no `/`, spaces, or `:`) is resolved from the process
# environment at startup. Example: `$LINEAR_API_KEY` reads env var LINEAR_API_KEY.
# Unset variables resolve to empty string with a warning.
# ═══════════════════════════════════════════════════════════════════════════════

# ─── Doctor CLI reference ─────────────────────────────────────────────────────
# Run: symphony doctor [WORKFLOW.md]
#
# Doctor validates this workflow before runtime and prints traffic-light lines:
#   ✅ pass      — healthy
#   ⚠️ warning   — non-fatal issue to review
#   🚨 error     — fatal issue (doctor exits 1)
#   ⏭️ skipped   — check intentionally not run
#
# Current check groups:
#   - Config: parse + validate + env-var resolution + prompt file paths + Slack event names
#   - Tracker:
#       Linear: auth (viewer), project slug resolution, workflow state alignment, assignee lookup
#       GitHub: PAT auth, repo access, Projects v2 Status field check
#   - Backend: configured backend command present on PATH and responds to `--version`
#   - Workspace: root path writable/creatable, repo reference sanity, git strategy compatibility,
#                Docker daemon availability when isolation=docker
#   - Orphans: on-disk workspace directories that do not map to active tracker issues
#   - Triage (when triage.enabled): storage path writability + exclusive lock probe,
#     triage prompt file, intake/route labels, GitHub label presence, Projects v2
#     route states, and a deferred harness-auth isolation warning
#
# Notes:
#   - Exit code is 0 when no 🚨 errors are found, otherwise 1.
#   - SSH host reachability checks are currently reported as ⏭️ skipped (future work).
#   - Doctor is report-only (no auto-fix), except it may create `workspace.root`
#     when missing so writability can be validated.

# ─── Tracker ──────────────────────────────────────────────────────────────────
# Configures which issue tracker to poll and how to filter issues.
tracker:
  # Tracker backend: "linear" or "github".
  kind: linear

  # Tracker API token (supports $VAR indirection).
  # - Linear: personal API key (for example $LINEAR_API_KEY)
  # - GitHub: PAT (for example $GH_TOKEN or $GITHUB_TOKEN)
  #
  # GitHub token source order:
  #   1) tracker.api_key
  #   2) GH_TOKEN
  #   3) GITHUB_TOKEN
  #   4) `gh auth token` (local fallback only; requires `gh auth login`)
  #
  # GitHub classic PAT scopes required for end-to-end Symphony runs:
  #   repo, project, workflow, read:org, read:discussion
  #
  # `repo`/`project` are required for issue and Projects v2 state management.
  # `workflow` is required for workflow/Actions operations. `read:org` and
  # `read:discussion` are required by GitHub PR review/comment GraphQL fields
  # used during Agent Review and CI inspection.
  #
  # For cloud/VPS, prefer explicit env secrets (GH_TOKEN/GITHUB_TOKEN).
  api_key: $LINEAR_API_KEY

  # Optional tracker endpoint override.
  # - Linear default: https://api.linear.app/graphql
  # - GitHub default: https://api.github.com
  # endpoint: https://api.linear.app/graphql

  # Linear-only: project URL slug or slugId from
  # https://linear.app/<workspace>/project/<slug>
  project_slug: "89d4761fddf0"

  # Linear-only: workspace slug used for dashboard project links.
  # When omitted, Symphony falls back to "kata-sh".
  # workspace_slug: kata-sh

  # GitHub-only: repository owner and repository name.
  # Required when kind: github.
  # repo_owner: kata-sh
  # repo_name: kata-mono

  # GitHub-only: Projects v2 project number.
  # Required when kind: github. State is read from the project's Status field.
  # github_project_owner_type chooses the GitHub Projects URL namespace:
  #   user -> https://github.com/users/<repo_owner>/projects/<number>
  #   org  -> https://github.com/orgs/<repo_owner>/projects/<number>
  # github_project_owner_type: org
  # github_project_number: 7

  # Optional: filter candidate issues to this assignee.
  # - Linear: username/display name/email/user id lookup
  # - GitHub: login match against assignee/assignees
  # When omitted, ALL issues in the project/repository matching active_states are eligible.
  # Supports $VAR indirection.
  # assignee: alice

  # Issue states eligible for dispatch. Issues in these states are candidates
  # for agent work. The orchestrator polls for issues in these states.
  # Default parser value: ["Todo", "In Progress"].
  # This template extends that set so the agent can run full review/merge loops.
  active_states:
    - Todo
    - In Progress
    - Agent Review
    - Merging
    - Rework

  # Issue states that mark work as complete. Issues reaching these states are
  # removed from the running/retry sets and counted as completed.
  terminal_states:
    - Closed
    - Cancelled
    - Canceled
    - Duplicate
    - Done

  # Optional: labels that disqualify an issue from dispatch entirely.
  # Any issue carrying at least one of these labels is silently skipped,
  # regardless of its state. Matching is case-insensitive.
  # Use ["kata:task"] when running Symphony against a Kata-planned project
  # to prevent sub-tasks from being dispatched as independent workers —
  # sub-tasks should only be executed by the parent slice worker.
  # Default: [] (no issues excluded by label).
  # exclude_labels:
  #   - kata:task

# GitHub Projects v2 example:
# tracker:
#   kind: github
#   api_key: $GH_TOKEN
#   repo_owner: kata-sh
#   repo_name: kata-mono
#   github_project_owner_type: org
#   github_project_number: 7
#   active_states:
#     - Todo
#     - In Progress
#   terminal_states:
#     - Done

# ─── Polling ──────────────────────────────────────────────────────────────────
# Controls how frequently Symphony polls the tracker for new/changed issues.
polling:
  # Milliseconds between poll cycles. Lower = more responsive, more API calls.
  interval_ms: 30000

# ─── Shared Context (Inter-worker coordination, M002/S06) ───────────────────
# Ephemeral in-memory context entries shared across worker sessions.
# Restarts clear this store by design.
shared_context:
  # Default TTL for new context entries in milliseconds.
  # Entries older than ttl_ms are pruned automatically each poll cycle.
  # Default: 86400000 (24h)
  ttl_ms: 86400000

  # Maximum number of entries retained in memory.
  # When exceeded, the oldest entries are evicted first.
  # Default: 100
  max_entries: 100

# ─── Workspace ────────────────────────────────────────────────────────────────
# Configures how agent workspaces are created and managed.
# Each dispatched issue gets its own workspace directory.
workspace:
  # Root directory for all workspaces. Each issue gets a subdirectory.
  # Supports ~ tilde expansion and $VAR indirection.
  root: ~/symphony-workspaces

  # Repository to bootstrap into each workspace. Can be:
  #   - A remote URL (https:// or git@): cloned from the remote
  #   - A local path: cloned locally (fast, hard-links .git objects)
  # Supports $VAR indirection and ~ tilde expansion.
  repo: https://github.com/gannonh/kata.git

  # Git bootstrap strategy (replaces the old `strategy` field):
  #   - "auto" (default): clone-remote if repo is a URL, clone-local if repo is a local path
  #   - "clone-local": `git clone --local <path> .` — fast (hard-links), inherits remotes
  #   - "clone-remote": `git clone <url> . --single-branch` — full network clone
  #   - "worktree": `git worktree add` from the source repo
  #     - Requires `repo` to be a local path
  #     - Lightweight — shares .git objects with source
  #     - Cleanup runs `git worktree remove`
  #
  # The old `strategy: clone | worktree` field is still accepted with a
  # deprecation warning. `clone` maps to `auto`, `worktree` stays `worktree`.
  # If both `strategy` and `git_strategy` are set, `git_strategy` wins.
  git_strategy: auto

  # Workspace isolation mode:
  #   - "local" (default): run agent directly on the host
  #   - "docker": run agent in an ephemeral container
  # Docker mode requires git_strategy "auto" or "clone-remote" (clone-local and
  # worktree need host filesystem access). repo must be a remote URL.
  isolation: local

  # Prefix for auto-created issue branches: <prefix>/<issue-identifier>
  # Example: symphony/KAT-814
  branch_prefix: symphony

  # Branch to clone/base off for clone-based strategies.
  # When set, clone uses `--branch <clone_branch>`.
  # When omitted, clone uses the repo's default branch.
  # Supports $VAR indirection.
  clone_branch: main

  # Base branch for workflow merge/rebase/pull operations.
  # Prompt instructions can reference this as `{{ workspace.base_branch }}`.
  # Default: main.
  base_branch: main

  # Whether to auto-remove workspaces when their issue reaches a terminal state.
  # When true, runs `before_remove` hook then deletes the workspace directory.
  # Default: false (workspaces persist for debugging).
  # cleanup_on_done: false

  # Docker-specific options (used when `workspace.isolation: docker`).
  # If omitted, these defaults are applied automatically.
  docker:
    # Base image used for worker containers.
    # Bundled worker image defaults to non-root user `node` (home `/home/node`).
    # Default: symphony-worker:latest
    image: symphony-worker:latest

    # Optional setup script path on the host. Symphony hashes the script
    # content and caches a derived image layer.
    # setup: docker/setups/rust.sh

    # Codex auth mode inside the worker container.
    # Interactive browser login is not available inside containers —
    # use OPENAI_API_KEY in .env or mount an existing auth file.
    #   - auto  (default): OPENAI_API_KEY if set, else stage host ~/.codex/auth.json to $HOME/.codex/auth.json in-container
    #   - env:   force OPENAI_API_KEY (simplest for Docker)
    #   - mount: force host ~/.codex/auth.json -> $HOME/.codex/auth.json in-container
    # codex_auth: auto

    # Extra env vars passed at `docker run` time.
    # env:
    #   - CARGO_HOME=/usr/local/cargo

    # Extra bind mounts passed at `docker run` time.
    # volumes:
    #   - ~/.ssh:/home/node/.ssh:ro

# ─── Hooks ────────────────────────────────────────────────────────────────────
# Shell commands run at workspace lifecycle events. Hook commands execute with
# cwd set to this WORKFLOW.md file's directory, so relative paths in hook
# commands resolve like prompt paths. All hooks receive these environment
# variables:
#   SYMPHONY_ISSUE_ID          — tracker issue ID
#   SYMPHONY_ISSUE_IDENTIFIER  — e.g. KAT-814
#   SYMPHONY_ISSUE_TITLE       — issue title text
#   SYMPHONY_WORKSPACE_PATH    — absolute path to the workspace directory
hooks:
  # Timeout for each hook invocation in milliseconds.
  timeout_ms: 120000

  # Run after workspace directory is created (after git bootstrap).
  # after_create: echo "Workspace created for $SYMPHONY_ISSUE_IDENTIFIER"

  # Run before the Codex session starts.
  # before_run: echo "Starting session"

  # Run after the Codex session ends (success or failure).
  # after_run: echo "Session complete"

  # Run before workspace directory is removed (cleanup_on_done or manual).
  # before_remove: echo "Cleaning up $SYMPHONY_ISSUE_IDENTIFIER"

# ─── Agent ────────────────────────────────────────────────────────────────────
# Controls agent session behavior and concurrency.
agent:
  # Maximum number of agent sessions running simultaneously.
  # New dispatches are held until a slot opens.
  # Default parser value: 10.
  max_concurrent_agents: 1

  # Maximum turns per session before the run is
  # considered complete for a single worker attempt.
  max_turns: 20

  # Maximum exponential back-off delay (ms) between retries on failure.
  # max_retry_backoff_ms: 300000

  # Timeout (ms) to wait for a human escalation response before the worker
  # falls back to auto-cancel/reject behavior.
  # escalation_timeout_ms: 300000

  # Worker runner name. This chooses how Symphony speaks to the worker process.
  # It is separate from tracker/backend-state operations.
  #   - pi: launch the Pi RPC runtime
  #   - codex: launch a Codex app-server process
  name: pi

  # Command to start the selected worker runner. Can be a shell-style string
  # or a list. Include the runner mode in the command. For local workers,
  # Symphony starts the runner with the workspace as the process cwd.
  command: pi --mode rpc

  # Codex example:
  # name: codex
  # command: codex --config shell_environment_policy.inherit=all --config model_reasoning_effort=xhigh --model gpt-5.3-codex app-server

  # Model passed to runners that support a model flag.
  model: anthropic/claude-opus-4-6

  # Per-state model overrides. Keys are workflow state names
  # (case-insensitive). If a state isn't listed, falls back to `model`.
  # model_by_state:
  #   Agent Review: anthropic/claude-sonnet-4-6
  #   Merging: anthropic/claude-sonnet-4-6

  # Whether to pass `--no-session` for `name: pi` (default: true).
  no_session: true

  # Optional file path passed via `--append-system-prompt` for `name: pi`.
  # append_system_prompt: /absolute/path/to/prompt.md

  # Hard timeout per Codex turn in milliseconds (default: 3600000 = 1 hour).
  # turn_timeout_ms: 3600000

  # Time (ms) before a non-progressing session is considered stalled.
  # Reset on each agent event. Set high for long builds (e.g. cargo test).
  # Default parser value: 300000.
  stall_timeout_ms: 900000

  # Timeout waiting for Codex process output in milliseconds.
  # read_timeout_ms: 5000

  # Approval policy for sandbox actions.
  # Default parser value: reject sandbox/rules/MCP elicitations.
  # `never` enables unattended auto-approval behavior for this workflow.
  approval_policy: never

  # Sandbox mode for the agent thread.
  # Default parser value: workspace-write.
  thread_sandbox: danger-full-access

  # Per-turn sandbox policy override.
  # Default parser value: unset.
  turn_sandbox_policy:
    type: dangerFullAccess

  # Timeout waiting for stdout lines in milliseconds.
  # Default parser value: 5000.
  read_timeout_ms: 5000

  # Time (ms) before a non-progressing session is considered stalled.
  # Default parser value: 300000.
  stall_timeout_ms: 300000

# ─── Supervisor (M002/S07) ────────────────────────────────────────────────────
# Optional autonomous supervisor loop for cross-worker orchestration.
supervisor:
  # Enable supervisor runtime. Default parser value: false.
  enabled: false

  # Optional model identifier for future model-backed supervisor reasoning.
  # Defaults to agent.model when omitted.
  # model: anthropic/claude-sonnet-4-6

  # Minimum milliseconds between steer actions for the same worker issue.
  # Default parser value: 120000.
  steer_cooldown_ms: 120000

# ─── Worker (SSH) ─────────────────────────────────────────────────────────────
# Distribute agent sessions across remote SSH hosts.
# When ssh_hosts is empty (default), all sessions run locally.
# worker:
#   ssh_hosts:
#     - worker1.example.com            # default port 22
#     - worker2.example.com:2222       # custom port
#     - alice@worker3.example.com      # custom user
#     - "[::1]:2222"                   # IPv6 with port
#   max_concurrent_agents_per_host: 3

# ─── Server ───────────────────────────────────────────────────────────────────
# HTTP dashboard and JSON API. Serves live orchestrator state.
server:
  # Port to bind. Also settable via --port CLI flag (CLI takes precedence).
  # CLI default is currently 8080.
  port: 8080

  # Bind address. Use "0.0.0.0" to expose on all interfaces.
  host: "127.0.0.1"

# ─── Event Stream WebSocket Contract (S01 + S03 escalation events) ───────────
# Endpoints:
#   GET  /api/v1/events
#   GET  /api/v1/escalations
#   POST /api/v1/escalations/:request_id/respond
#
# Query params for /api/v1/events (all optional):
#   issue=KAT-920,KAT-921
#   type=worker,tool
#   severity=info,error
#
# Filter semantics are deterministic:
#   - OR within each field (issue list, type list, severity list)
#   - AND across fields
#
# Invalid filters return HTTP 400 with machine-readable payload:
# {
#   "error": {
#     "code": "invalid_filter",
#     "status": 400,
#     "message": "...",
#     "details": { "field": "type", "value": "wat" }
#   }
# }
#
# Stream payload: one JSON envelope per message.
#
# {
#   "version": "v1",
#   "sequence": 42,
#   "timestamp": "2026-03-27T02:00:00Z",
#   "kind": "worker",            # snapshot | runtime | worker | tool | heartbeat |
#                                # escalation_created | escalation_responded |
#                                # escalation_timed_out | escalation_cancelled
#   "severity": "info",          # debug | info | warn | error
#   "issue": "KAT-920",          # optional issue identifier
#   "event": "worker_completed", # stable event name
#   "payload": { ... }             # sanitized summary payload (no secrets/raw prompts)
# }
#
# Connection lifecycle:
#   1) First envelope is always `kind=snapshot` (bootstrap state)
#   2) Periodic `kind=heartbeat` envelopes keep the stream healthy
#   3) Slow consumers are disconnected with close reason `backpressure`
#   4) Graceful shutdown uses close reason `server_shutdown`
#
# Example websocket query:
#   websocat "ws://127.0.0.1:8080/api/v1/events?issue=KAT-920&type=worker,tool,escalation_created&severity=info"
#
# Example escalation response:
#   curl -sS -X POST "http://127.0.0.1:8080/api/v1/escalations/escalation-123/respond" \
#     -H 'content-type: application/json' \
#     -d '{"response":{"confirmed":true},"responder_id":"operator-1"}'

# ─── Notifications ─────────────────────────────────────────────────────────────
# Optional webhook notifications for issue state transitions and runtime events.
# Messages include a clickable link to the Linear issue.
# notifications:
#   slack:
#     # Webhook URL or $ENV_VAR reference.
#     webhook_url: $SLACK_WEBHOOK_URL
#
#     # Event filters (case-insensitive, normalized to lowercase):
#     #
#     # State transitions:
#     #   todo, in_progress, agent_review, human_review,
#     #   merging, rework, done, closed, cancelled
#     #
#     # Runtime events:
#     #   stalled  — worker exceeded stall timeout
#     #   failed   — non-stall worker failure during execution
#     #
#     # Wildcard:
#     #   all      — subscribe to every event
#     #
#     # Empty list means no notifications are sent.
#     events:
#       - all

# ─── Storage (factory-run durability) ─────────────────────────────────────────
# Optional SQLite path for durable factory runs (A1 triage and later stages).
# When `storage.path` is omitted, Symphony uses a namespaced path under the
# platform data directory:
#   <data>/symphony/triage/<forge>/<owner>/<repo>/triage.sqlite3
#
# `busy_timeout_ms` controls SQLite busy waiting (default 5000). An adjacent
# `.lock` file provides exclusive ownership for the running Symphony process.
#
# storage:
#   path: $SYMPHONY_STATE_PATH
#   busy_timeout_ms: 5000

# ─── Triage (A1 GitHub issue triage) ──────────────────────────────────────────
# GitHub Projects v2 only. When `enabled: false` (default), Symphony skips triage
# intake and leaves implementation polling unchanged.
#
# Modes:
#   preview   — durable artifact + diagnostic comment; no route label/state mutation
#   automatic — publishes the chosen route (label, optional project state, intake removal)
#
# Preview is the safe rollout path: collect metrics and review artifacts, then
# promote to automatic after doctor checks and sample agreement look healthy.
#
# `prompt` is resolved relative to this WORKFLOW.md directory (same anchor as
# `prompts.*` and hooks). `workspace.root` / local `workspace.repo` stay
# relative to the process cwd (typically the repository root), and are
# canonicalized before triage clones so values like `repo: .` stay stable.
# Route labels must
# be distinct and must not equal `intake_label`. Optional `state` values must exist
# on the Projects v2 Status field.
#
# Restart recovery: each poll interrupts attempts whose lease went stale, then
# reclaims them. A recorded triage child is terminated only when its pid,
# process group, and OS start token still match, so a reused PID is never
# signalled. The recorded executable is diagnostic because a launcher may
# legitimately replace itself through `exec`; otherwise Symphony skips the
# signal. The attempt's disposable directory is removed before retry, and the attempt stays
# interrupted so it still counts against `max_attempts`. Late output from an
# interrupted attempt is rejected rather than recorded as success.
#
# Agreement measurement: after a route is published, Symphony re-reads the issue
# from its durable record and compares current labels against the five route
# labels stored on that publication, so reloading this file with a different
# mapping cannot reinterpret an older publication. Replacing the route label
# emits `triage_route_corrected` once per artifact and observed route. Zero or
# several route labels is recorded as a durable `triage_route_consistency`
# diagnostic instead, and is not counted as disagreement.
#
# HTTP (when the observability server is enabled):
#   GET /api/v1/factory-runs/{run_id}
#   GET /api/v1/factory-runs?issue={issue_identifier}
#   GET /api/v1/factory-runs/metrics?stage=triage   # includes correction_count / correction_rate
#
# triage:
#   enabled: false
#   mode: preview
#   intake_label: needs-triage
#   prompt: prompts/triage.md
#   # model: provider/model-name  # Pi only
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

# ─── Specification stage (A2) ─────────────────────────────────────────────────
# GitHub Projects v2 only. An open project issue carrying `intake_label` runs
# isolated draft → adversarial review → revise turns. Every completed attempt
# publishes one immutable, monotonically versioned artifact in one owned issue
# comment. Apply `spec-revise` after adding feedback to produce the next version,
# or `spec-approved` to pin the current version and apply the implementation route.
# Decision labels, intake, and approval labels must be distinct. When triage is
# enabled, `triage.routes.spec.label` must equal `spec.intake_label`.
#
# HTTP additions:
#   GET /api/v1/factory-runs/{run_id} # includes spec versions and per-turn data
#   GET /api/v1/factory-runs/{run_id}/artifacts/{artifact_id}
#   GET /api/v1/factory-runs/metrics?stage=spec
#
# spec:
#   enabled: false
#   intake_label: ready-to-spec
#   prompts:
#     draft: prompts/spec-draft.md
#     review: prompts/spec-review.md
#     revise: prompts/spec-revise.md
#   # model: provider/model-name
#   # review_model: provider/model-name
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

# ─── Implementation stage (A3) ────────────────────────────────────────────────
# GitHub Projects v2 only. Admits factory runs with a terminal A2 approval
# publication and pinned approved_artifact_id/version. Workers implement in
# credential-free local or Docker isolation, run repository-owned validation,
# and store a verified change bundle. Preview mode publishes one owned issue
# comment with no remote branch/PR or tracker mutation. Automatic mode (PR2)
# pushes a draft PR and advances completion_route.state.
#
# Requires `spec.enabled` so the A2 approval path exists. Validation must list
# 1–20 uniquely named blocking commands. `implementation.model` is rejected for
# Codex backends.
#
# HTTP additions:
#   GET /api/v1/factory-runs/{run_id} # includes implementation status when present
#   GET /api/v1/factory-runs/metrics?stage=implementation
#
# implementation:
#   enabled: false
#   mode: preview
#   prompt: prompts/implementation.md
#   repair_prompt: prompts/implementation-repair.md
#   # model: provider/model-name
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

# ─── Prompts (per-state prompt injection) ─────────────────────────────────────
# Optional. When configured, the orchestrator selects a prompt template based on
# the issue's tracker state at dispatch time instead of using the markdown body
# below the --- delimiter. Each file is a Liquid template with access to
# {{ issue.* }}, {{ attempt }}, and {{ workspace.base_branch }}.
#
# File paths are resolved relative to this WORKFLOW.md file's directory.
#
# When this section is absent, the entire markdown body after --- is used as
# the prompt for all states (backward compatible with existing workflows).
#
# Prompt files are concatenated in order: system + repo + shared (legacy) + state.
# Each pair is separated by `---`.
#
# `system` — repo-agnostic agent identity and tool guidance, injected every turn.
# `repo`   — repository-specific context (build commands, layout), injected every turn.
# `shared` — legacy single-file preamble. Superseded by `system` + `repo` but
#            still honoured for backward compatibility.
#
# State keys are matched case-insensitively.
#
# Template variables available in per-state prompts:
#   {{ issue.identifier }}        — e.g. "KAT-928"
#   {{ issue.title }}             — issue title
#   {{ issue.state }}             — current tracker state name
#   {{ issue.labels }}            — comma-separated label names
#   {{ issue.description }}       — issue body (may be empty)
#   {{ issue.url }}               — tracker issue URL
#   {{ issue.children_count }}    — number of child sub-issues (0 for flat tickets)
#   {{ issue.parent_identifier }} — parent issue identifier (nil for non-sub-issues)
#   {{ attempt }}                 — retry attempt number (nil on first dispatch)
#   {{ workspace.base_branch }}   — configured base branch (default: "main")
#
# prompts:
#   system: prompts/system.md
#   repo: prompts/repo.md
#   by_state:
#     Todo: prompts/in-progress.md
#     In Progress: prompts/in-progress.md
#     Agent Review: prompts/agent-review.md
#     Merging: prompts/merging.md
#     Rework: prompts/rework.md
#   default: prompts/in-progress.md
---
---

<!-- ═══════════════════════════════════════════════════════════════════════
  PROMPT TEMPLATE BODY
  ═══════════════════════════════════════════════════════════════════════

  With per-state prompt injection (the `prompts:` config section above),
  the prompt body after this --- delimiter is NOT used. The orchestrator
  reads prompt files from the `prompts/` directory instead.

  If you are NOT using per-state prompts, put your monolith prompt
  template here — everything below --- becomes the Liquid template
  rendered for every agent session regardless of issue state.

  See the `prompts/` directory for the per-state prompt files:
    prompts/system.md          — agent identity, tool guidance (repo-agnostic)
    prompts/repo.md            — repo-specific context (build, layout, conventions)
    prompts/in-progress.md     — implement, test, push, open PR
    prompts/agent-review.md    — address PR feedback
    prompts/merging.md         — land the PR
    prompts/rework.md          — close PR, fresh start
  ═══════════════════════════════════════════════════════════════════════ -->
