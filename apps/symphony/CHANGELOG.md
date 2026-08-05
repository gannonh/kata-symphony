# Changelog

## Unreleased

### Features

- **A5 verification evidence preview** — a typed `verification` stage runs configured blocking commands against the exact A4-reviewed PR head in a credential-free bundle workspace, collects bounded digest-addressed evidence, invokes a read-only verifier against the pinned spec/implementation/review artifacts, and computes a deterministic gate the verifier cannot override. Preview mode publishes one owned issue comment. A4 review cycles are keyed by head and base so a base-only change opens a fresh review cycle while preserving every existing artifact, finding, and publication reference.

## 2.3.0

### Features

- **Project-local Symphony home** — `symphony init` writes starter workflow, prompts, `.env.example`, and reference docs under `.symphony/`, and no-arg runs resolve `.symphony/WORKFLOW.md` when present.
- **Prompt-driven worker contract** — worker sessions receive `SYMPHONY_BIN` and `SYMPHONY_WORKFLOW_PATH` so prompts can call `symphony helper` directly.
- **Backend-neutral helper CLI** — added helper operations for issue reads, child listing, comment upsert, state updates, follow-up issue creation, and document read/write.
- **Linear helper parity** — Linear workflows now support helper-backed issue reads, comments, follow-up issues, and state updates.
- **TUI and dashboard visibility** — added retry queue counts, throughput sparkline, project URL, model column, tracker state display, pinned errors, and stronger status coloring.

### Improvements

- **Symphony logging controls** — added `SYMPHONY_LOG` and `SYMPHONY_LOG_ROOT`, with `RUST_LOG` retained as a fallback.
- **Agent config documentation** — consolidated runtime config around `agent.name`, `agent.command`, model, and timeout fields.
- **Workspace refresh hardening** — clean worktree workspaces can fast-forward from `clone_branch`, while stale dirty or diverged workspaces are reported clearly.

### Bug Fixes

- **GitHub Projects v2 state refresh** — issue refresh and candidate fetch now derive tracker state from project item status consistently.
- **Session continuation safety** — active-state changes now end the current worker session so Symphony can redispatch with the correct prompt.
- **Retry token safety** — stale retries are suppressed and retry context is exposed in snapshots.
- **Helper input isolation** — starter prompts require unique helper input files to avoid shared temp-file collisions.

## 2.2.2

### Improvements

- **Align GitHub execution contract with Kata CLI semantics** — harmonized state/transition behavior and workspace prompt shaping so Symphony and Kata CLI interpret GitHub issue workflows consistently.
- **Backend-neutral worker contract support** — updated prompts, adapter/config plumbing, and tests to operate cleanly across Linear and GitHub backends via shared `kata_*` abstractions.

## 2.2.1

### Bug Fixes

- **Gate Agent Review on an actual open PR** — before dispatching or retrying an `Agent Review` run, Symphony now verifies that the current branch exists on `origin` and that `gh pr view` returns an open PR for that branch.
- **Self-heal invalid Agent Review states** — if an issue enters `Agent Review` without an open PR, Symphony moves it back to `In Progress` and posts a note instead of launching a broken review session.
- **Harden Agent Review prompt assumptions** — the worker prompt now requires PR proof instead of assuming a PR already exists.

## 2.2.0 — GitHub Issues Backend

Full drop-in alternative to Linear as a tracker backend. Both label-based and Projects v2 state management modes supported.

### Features

- **GitHub Issues tracker backend** — `tracker.kind: github` enables Symphony to poll, dispatch, and reconcile against GitHub Issues instead of Linear. PAT authentication via `GH_TOKEN` / `GITHUB_TOKEN`.
- **Label-based state management** — Issues transition via label swaps (`symphony:todo` → `symphony:in-progress` → `symphony:done`). Configurable label prefix via `tracker.label_prefix`.
- **Projects v2 state management** — When `tracker.github_project_number` is set, Symphony reads and writes the board's Status field via GraphQL mutations. Auto-detects mode from config.
- **`tracker.exclude_labels`** — New config field to prevent issues with specific labels from being dispatched. Use `["kata:task"]` to block sub-task dispatch. Case-insensitive matching.
- **GitHub-specific `symphony doctor` checks** — PAT authentication, repo accessibility, Projects v2 project/status-field validation, and label existence checks.
- **Dashboard GitHub parity** — `#N` identifiers rendered as clickable links to `https://github.com/{owner}/{repo}/issues/N`. Project card links to GitHub. All surfaces (TUI, HTTP dashboard, Slack notifications, `/api/v1/state` JSON) show GitHub identifiers and URLs.
- **`tracker_project_url` generalization** — Renamed from `linear_project_url` in the orchestrator snapshot. Dispatches to GitHub or Linear URL format based on `tracker.kind`.
- **`GithubOrchestratorPort`** — Full `OrchestratorPort` implementation wired in `main.rs`. Port selection based on `tracker.kind` at startup.
- **Rate-limit-aware GitHub API client** — Tracks `X-RateLimit-Remaining` headers, logs warnings at 10% budget, delays requests when exhausted.
- **Reference WORKFLOW configs** — `docs/WORKFLOW-github-labels.md` and `docs/WORKFLOW-github-projects.md` for quick setup.

### Bug Fixes

- **Inter-turn issue state refresh** — Was hardcoded to `LinearClient`, causing 403 errors when running with GitHub tracker. Now uses `build_tracker_adapter()` which dispatches to the correct backend.
- **Projects v2 GraphQL partial errors** — GitHub returns errors for the `organization` path when the owner is a user account, but the `user` path succeeds. The `graphql_request` helper now only treats errors as fatal when `data` is absent.
- **`/symphony config` validator** — `$VAR` env references in `webhook_url` no longer trigger URL validation errors.
- **`/symphony config` module loading** — `config-parser.ts` and `config-writer.ts` now load `js-yaml` via `createRequire` instead of static imports, fixing load failures in agent context.
- **Blank label filtering** — `issue_has_excluded_label` now filters empty/whitespace labels from both the config and issue sides.

### Config

New `tracker` fields for GitHub mode:

| Field | Type | Description |
|-------|------|-------------|
| `tracker.kind` | `"linear"` \| `"github"` | Tracker backend selection |
| `tracker.repo_owner` | string | GitHub repository owner |
| `tracker.repo_name` | string | GitHub repository name |
| `tracker.github_project_number` | u64 | Projects v2 board number (omit for label mode) |
| `tracker.label_prefix` | string | Label prefix for state labels (default: `symphony`) |
| `tracker.exclude_labels` | string[] | Labels that disqualify issues from dispatch |

## 2.1.0

### Features

- **Skills injection into workspaces** — Symphony now auto-copies a `skills/` directory (sibling to WORKFLOW.md) into `.agents/skills/` in each worker workspace during bootstrap. Skills use a `sym-` namespace prefix to avoid collisions with user skills.
- **`symphony doctor` preflight diagnostics** — New `symphony doctor` command validates configuration, connectivity, and runtime prerequisites before starting the orchestrator.
- **Persistent worker error surfacing** — Worker failures are now propagated into orchestrator state and rendered in the TUI with 🚨 + red styling. The HTTP dashboard shows an Error column for running sessions. Rate-limit and usage-limit errors include retry-window hints when parseable, with 200-char truncation for redaction safety.
- **`POST /api/v1/steer` endpoint** — Live operator steering for active workers. Delivers guidance to running Kata RPC sessions via `follow_up` injection. Includes `SteerSender`/`SteerResult` types, `steer_channel()`, and full HTTP validation/error mapping.
- **`model_by_label` config** — Label-first model resolution parsed from WORKFLOW.md alongside `model_by_state`. Normalized to lowercase, takes priority over state-based and default model selection.

### Bug Fixes

- **S01 critical bug fixes (KAT-1652)** — `pi-agent` `stopReason: "error"` handling with rate-limit hint parsing; startup orphan workspace cleanup via `scan_workspace_root`; workpad duplication guard with search-before-create pattern and one-retry comment creation.
- **Project identifier preferences** — Updated project identifiers in CLI and Symphony configurations.

### Infrastructure

- **llvm-cov CI gate** — Added coverage gating and core edge-path coverage tests.

## 2.0.0 — Symphony ↔ Kata CLI Integration

Server/client architecture — Symphony is the server, Kata CLI is the client. Same planning artifacts, same Linear issues, same agent runtime at every level.

### WebSocket Event Stream API (S01)

- **`/api/v1/events` WebSocket endpoint** — real-time event stream with server-side filtering (`?issue=KAT-920&type=worker,tool&severity=error`). OR within fields, AND across fields.
- **21 event types** — worker lifecycle, tool execution, escalation, shared context, supervisor decisions.
- **Bootstrap snapshot** — new subscribers receive current state immediately, then live deltas.
- **Backpressure protection** — bounded per-client queues with deterministic close codes (`invalid_filter`, `backpressure`, `server_shutdown`).
- **Heartbeat keepalive** — 5-second heartbeat envelopes for connection health monitoring.

### Worker Escalation Protocol (S03)

- **Real-time human-in-the-loop** — workers can escalate ambiguous decisions instead of guessing or failing. Worker pauses, question routes to connected Kata CLI, human answers, worker resumes. No restart needed.
- **`POST /api/v1/escalations/:id/respond`** — HTTP endpoint for routing operator responses to waiting workers.
- **Configurable timeout** — `agent.escalation_timeout_ms` (default 5 min). On timeout, falls back to original cancel behavior.
- **TUI escalation indicators** — ⚠️ icon + question preview on workers with pending escalations.
- **Full lifecycle events** — `escalation_created`, `escalation_responded`, `escalation_timed_out`, `escalation_cancelled`.

### Inter-worker Context Sharing (S06)

- **`SharedContextStore`** — in-memory, scope-keyed store (project, milestone, label). Workers share decisions and patterns so parallel agents don't contradict each other.
- **Automatic prompt injection** — relevant context entries injected into worker prompts via `{{ shared_context }}` Liquid variable. Top 10 newest entries per scope.
- **HTTP API** — `GET/POST/DELETE /api/v1/context` for programmatic access. Entries have author, scope, content (max 500 chars), and configurable TTL.
- **Auto-pruning** — expired entries cleaned each poll cycle. Events emitted on write and expiry.
- **Ephemeral by design** — in-memory only, clears on restart. Linear is the durable store.

### Supervisor Agent (S07)

- **Autonomous orchestration intelligence** — AI agent watches the event stream and makes real-time decisions. Steers stuck workers, detects conflicts between parallel workers, recognizes systemic failure patterns.
- **Stuck worker detection** — repeated tool errors (3+), no file edits (5+ events), repeated test failures. Targeted steer messages based on the specific pattern.
- **Conflict detection** — overlapping file edits between concurrent workers, contradictory shared context entries. Coordinates via context writes or escalates to human.
- **Failure pattern recognition** — 2+ workers hitting the same normalized error triggers systemic alert with shared context warning and optional escalation.
- **Configurable** — `supervisor.enabled` (default false), `supervisor.model`, `supervisor.steer_cooldown_ms` (default 120s).
- **Kata CLI as runtime** — supervisor spawns as a Kata CLI agent session with a specialized system prompt, not embedded Rust logic.
- **Dashboard visibility** — TUI header shows supervisor status and action counts. HTTP dashboard shows full supervisor section.

### Bug Fixes

- **Console "Waiting" message persists after connection** — cleared when WebSocket connects.
- **Escalation listener stack trace on disconnect** — cleaned up error handling, no raw traces leaked to UI.

## 1.3.0 — Slack notifications, session state-change fix

### Slack Webhook Notifications (KAT-925)

- **`notifications` config section** — configure outbound Slack webhook notifications in WORKFLOW.md frontmatter.
- **All state transitions** — notifications for every issue state change: `todo`, `in_progress`, `agent_review`, `human_review`, `merging`, `rework`, `done`, `closed`, `cancelled`. Plus runtime events: `stalled`, `failed`. Use `all` to subscribe to everything.
- **Linear issue links** — messages include a clickable link to the Linear issue.
- **Fire-and-forget dispatch** — notifications are spawned as async tasks; failures are logged as warnings but never block the orchestrator.
- **Event filtering** — `notifications.slack.events` controls which events trigger messages. Unknown event names are rejected at config parse time.
- **Webhook URL redaction** — webhook URLs are never logged; request errors are sanitized to category labels (timeout/connect/request/status/transport).
- **`$ENV_VAR` support** — `webhook_url` supports environment variable indirection (e.g. `$SLACK_WEBHOOK_URL`).
- **Example workflow** — `docs/WORKFLOW-slack.md` provides a complete workflow template with notifications config.

### Critical Fix

- **Multi-turn session now ends when issue state changes** — when an agent moves an issue from one active state to another (e.g. `In Progress` → `Agent Review`), the session now ends so the orchestrator can re-dispatch with the correct per-state prompt. Previously the multi-turn loop continued with the stale prompt because both states were "active," which could lead to the agent taking unauthorized actions (like merging a PR) after running out of meaningful work.
- **Post-transition dispatched state** — the multi-turn loop now compares against the effective post-transition state (e.g. `In Progress` after `Todo` → `In Progress`), not the stale pre-transition state. Prevents false session stops on the normal Todo auto-transition.

## 1.2.0 — Per-state prompts, dependency ordering, live tool activity

### Per-State Prompt Injection

- **State-driven prompt selection** — orchestrator selects a focused prompt based on the issue's Linear state at dispatch time instead of sending one monolithic prompt. Agents receive only the instructions relevant to their current job.
- **`prompts` config section** — configure `shared`, `by_state`, and `default` prompt file paths in WORKFLOW.md frontmatter. Files are resolved relative to the workflow file.
- **Issue shape detection** — `in-progress.md` prompt uses `children_count` and `parent_identifier` to auto-detect flat tickets, Kata-planned slices, and individual tasks. One workflow handles all three.
- **Project-specific shared prompts** — `shared-symphony.md` (Rust/Cargo) and `shared-cli.md` (TypeScript/Bun) with repo-specific build/test/lint commands.
- **Backward compatible** — without a `prompts` section, the full markdown body after `---` is used as before.
- **Agent Review empty-comments guard** — agents don't advance to Human Review when no PR comments exist yet (reviewers may not have spun up).

### Issue Dependency Ordering (KAT-927)

- **Generalized blocker check** — `is_blocked_by_dependency()` replaces the Todo-only `todo_issue_blocked_by_non_terminal()`. Issues in any active state with non-terminal blockers are held in the queue.
- **Circular dependency detection** — direct A↔B cycles detected and logged as warnings; neither issue dispatched.
- **Cross-project blockers** — blockers with unknown state (cross-project) treated as non-blocking with a log warning.
- **Blocked section in TUI** — new "Blocked" section between Running Sessions and Retry Queue shows blocked issues with their blocker identifiers.
- **Blocked in HTTP dashboard and API** — `blocked` array in `/api/v1/state` JSON and HTML dashboard table.

### Live Tool Activity Stream (KAT-926)

- **Structured tool notification parsing** — `notification_event_summary()` parses `tool_start:`, `tool_end:`, `tool_error:` prefixed messages from the RPC bridge into structured event names and `<tool>: <args_preview>` messages.
- **Tool activity colors in TUI** — `tool_start` → green, `tool_end` → blue, `tool_error` → red status dots.

### Linear Query Enrichment

- **`children_count` and `parent_identifier`** on `Issue` — candidate and by-ID queries now fetch `children.nodes` and `parent.identifier` for issue shape detection.

### Workflow Management

- **Workflow files gitignored** — `WORKFLOW.md` and `WORKFLOW-*.md` at root are gitignored (contain local paths/credentials). Example workflows in `docs/`.
- **Plans in issue descriptions** — slice and task plans stored in Linear issue descriptions instead of separate LinearDocuments. Summaries as issue comments.
- **Workpad protocol improved** — agents must load all context before creating workpad; placeholder content forbidden.
- **Agent Review state transition fixed** — execution phase moves to Agent Review (not Human Review); section headers and instructions aligned.

### Documentation

- **WORKFLOW-REFERENCE.md** — added `prompts` config section with all template variables; removed stale monolith prompt body.
- **AGENTS.md** — added `prompts` config section, `prompt_builder` module in module map.
- **README** — updated with per-state prompt explanation and prompt file table.

## 1.1.0 — Kata CLI backend, per-state model selection, docs overhaul

### Kata CLI Backend (KAT-902, KAT-912)

- **Multi-model agent backend** — new `agent.backend: kata-cli` (aliases: `kata`, `pi`) spawns Kata CLI in RPC mode, enabling any model supported by pi-ai (Anthropic, OpenAI, Google, Mistral, Bedrock, Azure)
- **`kata_agent` config section** (alias: `pi_agent`) — configure command, model, timeouts for the Kata CLI backend
- **Codex backend preserved** — `agent.backend: codex` continues to work unchanged; both backends coexist
- **Backend rename** — `AgentBackend::Pi` → `AgentBackend::KataCli`; YAML accepts `kata-cli`, `kata`, `pi`

### Per-state Model Selection (KAT-914)

- **`model_by_state`** — assign different models to different Linear workflow states (e.g. Opus for implementation, Sonnet for review)
- **Model column in TUI and web dashboard** — active model visible for each running session
- **Centralized model resolver** — `PiAgentConfig::model_for_state()` ensures orchestrator display and RPC launch stay in sync

### RPC Bridge Fixes

- **Handshake timeout fix** — polling loops continue on chunk timeouts instead of failing when Kata CLI takes >2s to start
- **EOF/IO error propagation** — `read_poll_line` helper distinguishes timeouts from subprocess crashes; EOF and IO errors propagate immediately instead of hot-spinning until deadline

### Removed

- **`max_concurrent_agents_by_state`** — removed from code, config, tests, and docs (feature had no valid use case)

### Docker

- **Dockerfile.symphony fixes** — added OpenSSL dev libs to builder stage, pinned `rust:slim-bookworm` to match runtime glibc, removed invalid `--host` CLI flag from CMD, added `--no-tui` for headless container operation
- **Docker Compose fixes** — fixed COPY paths for compose build context, removed obsolete `version` key, workflow file now mounts from docker/ directory
- **WORKFLOW-docker.md** — new ready-to-edit template pre-configured for Docker isolation, lives in docker/ directory alongside .env and compose file
- **Docker auth error message** — now tells users to set `OPENAI_API_KEY` instead of referencing non-existent `codex auth` command (interactive login unavailable in containers)
- **Non-root worker containers (KAT-903)** — worker image runs as dedicated non-root user (`node`) by default, auth mount resolves container home path dynamically instead of hardcoding `/root`, setup scripts run as root then restore non-root default user

### Documentation

- **README rewrite** — separated three deployment modes (local, Docker-isolated workers, server deployment), added prerequisites section, explicit quick start with .env.example, clear Docker mode explanation ("you don't manage containers"), CLI flags table
- **WORKFLOW-REFERENCE.md** — corrected Docker git_strategy constraints (only auto/clone-remote), clarified auth modes for containers
- **WORKFLOW-cli.md** — fixed Linear document scoping for slice docs (attached to slice issue, not project) to prevent namespace collisions across milestones
- **Workflow file renames** — `WORKFLOW.md` → `WORKFLOW-symphony.md`, `cli-WORKFLOW.md` → `WORKFLOW-cli.md`

### Fixes

- **go.sh setup script** — fixed checksum verification (Go doesn't serve per-file .sha256 URLs, now fetches from JSON release API)

## 1.0.1 — TUI/dashboard polish + post-patch verification

### Runtime and UX

- **TUI default-on behavior** — terminal dashboard is enabled by default; `--no-tui` is the explicit opt-out. Legacy `--tui` remains accepted as a no-op for compatibility.
- **Throughput sparkline** — TUI summary now includes a compact throughput graph for recent token rate changes.
- **Color-coded status dots** — running-session state indicators use event/staleness-aware colors for faster scanability.
- **Linear project URL visibility** — TUI summary and HTTP dashboard both surface the configured Linear project link.
- **Session-only completed list** — startup terminal-issue discovery no longer pollutes the per-session completed issues list.
- **Startup banner consistency** — banner version line is emitted from the crate package version.

### Verification and docs

- **Quality gate rerun** — validated with `cargo test`, `cargo clippy -- -D warnings`, and `cargo build --release`.
- **Documentation sync** — refreshed README, AGENTS architecture notes, and WORKFLOW reference to match current CLI/TUI/dashboard behavior.

## 1.0.0 — MVP Release

First public release. Symphony is a headless orchestrator that polls Linear for issues, dispatches parallel agent sessions, and manages the full ticket lifecycle autonomously.

### Core

- **Orchestrator runtime** — poll-dispatch-reconcile loop with configurable concurrency, priority-based dispatch, dependency graph awareness, and exponential backoff retries
- **Multi-turn sessions** — agents continue on the same Codex thread across turns, preserving conversation history. Issue state checked between turns; terminal state stops immediately.
- **Real-time event streaming** — events flow from workers to the orchestrator as they happen via mpsc channel. Dashboard updates live, stall detection uses real activity timestamps.
- **Dynamic config reload** — WORKFLOW.md changes take effect without restart via `Arc<WorkflowStore>`
- **Full PR lifecycle** — agents create PRs, address review feedback (address-comments skill), resolve comment threads, and merge (land skill)
- **Agent Review loop** — agents self-route to Agent Review after opening a PR, address all bot/reviewer comments, then move to Human Review when clean

### Workspace

- **Git strategies** — `clone-local` (fast, hard-links, inherits remotes), `clone-remote` (network clone), `worktree` (lightweight, shared .git), `auto` (picks based on repo URL vs path)
- **Workspace cleanup** — `cleanup_on_done: true` auto-removes workspaces when issues reach terminal state. Worktree strategy runs `git worktree remove`.
- **Branch prefix** — configurable `branch_prefix` for auto-created issue branches
- **Base branch** — `workspace.base_branch` exposed as Liquid template variable in prompts
- **Docker isolation stub** — `isolation: docker` accepted in config (implementation in next release)

### Linear Integration

- **Candidate polling** — filters by project, active states, assignee, priorities, and blocked-by dependencies
- **State writeback** — moves Todo → In Progress on dispatch; preserves other active states (Agent Review, Merging, Rework)
- **Assignee resolution** — `tracker.assignee` accepts `me`, username, email, or UUID
- **GraphQL skill** — bundled Linear skill with correct query patterns injected into agent context

### Dashboard

- **HTTP dashboard** — live session table, token summary (input/output/total), retry queue, completed issues (identifier + title + date), polling stats, rate limits. Auto-refreshes every 2 seconds.
- **TUI dashboard** — `--tui` flag activates Ratatui terminal UI. Running sessions with turn count, last activity, per-session tokens. Coexists with HTTP dashboard.
- **Per-session observability** — turn count, last activity timestamp, per-session token usage on RunAttempt

### Observability

- **Rotating log files** — `--logs-root` writes structured JSON logs to disk with rotation. Stdout shows startup banner only when log files are configured.
- **Startup banner** — clean summary on launch: dashboard URL, log path, project, workers, polling interval
- **Structured logging** — all events emitted as structured JSON via `tracing`

### Workflow

- **Multiple workflow files** — different projects use different workflows. `symphony WORKFLOW.md` vs `symphony cli-WORKFLOW.md`
- **Slice-aware workflow** — `cli-WORKFLOW.md` supports parent/child issue dispatch with context loading protocol (project → milestone → slice → task plans)
- **Skills** — bundled Codex skills: linear, commit, push, pull, land, address-comments, fix-ci, debug
- **Prompt template** — Liquid templates with `{{ issue.identifier }}`, `{{ issue.title }}`, `{{ workspace.base_branch }}`, `{{ attempt }}`

### SSH

- **Remote worker pools** — distribute sessions across SSH hosts with per-host concurrency caps
- **Host selection** — least-loaded eligible host with preference for prior host on retry

### Configuration

- **WORKFLOW.md** — YAML front-matter for all settings, markdown body for agent prompt template
- **Reference template** — `docs/WORKFLOW-REFERENCE.md` with all settings fully documented
- **Environment variable indirection** — `$VAR` syntax in config fields resolves from process environment
- **Hot reload** — config changes take effect without restart

### Testing

- 290 tests across 12 test harnesses
- Unit, integration, and conformance tests
- Clippy clean with `-D warnings`
