---
type: Spec
title: Pi Symphony Extension Design
description: Design for a Pi extension to initialize, launch, monitor, and steer Symphony.
tags: [symphony, pi, extension]
timestamp: 2026-07-15T20:00:00Z
---

# Pi Symphony Extension Design

Date: 2026-05-14
Package: `@kata-sh/pi-symphony-extension`
Source: `apps/symphony/pi-extension`

Related: [Wave 4 Shared Context and Diagnostics Plan](/superpowers/plans/_archive/2026-05-17-wave-4-symphony-shared-context-diagnostics.md). Roadmap: [specs/index.md](/specs/index.md).

## Goal

Create a Pi extension that lets users initialize, launch, monitor, and steer Symphony from Pi. The extension provides direct slash commands, LLM-callable tools, and a Pi-native dashboard backed by Symphony's HTTP API.

## Scope

In scope:

- Initialize Symphony project config with `symphony init`.
- Run `symphony doctor` from Pi.
- Start Symphony from Pi's current working directory as a headless child process with `--no-tui`.
- Attach to an already-running Symphony HTTP server.
- Render a Pi-native dashboard that reaches full HTTP dashboard parity through vertical slices.
- Monitor workers, retry queue, blocked issues, completed issues, pending escalations, shared context, polling, rate limits, token totals, and events.
- Steer active Symphony workers through `POST /api/v1/steer`.
- Package the extension for Pi distribution as `@kata-sh/pi-symphony-extension`.

Out of scope:

- Replacing Symphony's native Ratatui TUI.
- Changing Symphony's worker orchestration model.
- Requiring Kata CLI to use the extension.
- Stopping externally-started Symphony processes.

## Architecture

The extension is a self-contained Pi package source under `apps/symphony/pi-extension`.

### Extension entrypoint

The entrypoint registers commands, tools, and lifecycle handlers. It owns user-facing status, command dispatch, dashboard launch, and shutdown cleanup.

### SymphonyBinaryResolver

The resolver finds the Symphony binary in this order:

1. `SYMPHONY_BIN`
2. Repo-local `apps/symphony/target/release/symphony`
3. `symphony` on `PATH`

If the resolver cannot find a binary, it prompts the user for an absolute path, validates the path, and persists it as extension state.

### SymphonyProcessManager

The process manager starts Symphony from Pi's current working directory with `--no-tui`. It passes an optional workflow path when the user supplies one. It records whether the extension owns the child process, detects the HTTP binding from startup output and workflow/default configuration, and stops only owned child processes.

### SymphonyHttpClient

The HTTP client wraps Symphony API calls:

- `GET /api/v1/state`
- `GET /api/v1/events`
- `POST /api/v1/refresh`
- `POST /api/v1/steer`
- `GET /api/v1/escalations`
- `POST /api/v1/escalations/:request_id/respond`
- `GET`, `POST`, and `DELETE /api/v1/context`

It normalizes connection failures, non-Symphony responses, invalid JSON, and Symphony API error envelopes into typed extension errors.

### SymphonyDashboard

The dashboard is a Pi-native TUI component. It reads durable state from `GET /api/v1/state`, updates recent activity from the event stream when available, and sends control actions through the HTTP API. It closes with `q` or Escape and leaves Symphony running unless the user explicitly stops an owned process.

### ExtensionState

State tracks:

- Resolved or user-provided binary path.
- Attached base URL.
- Owned process metadata.
- Dashboard preferences.
- `stopOwnedOnShutdown`, defaulting to `true`.
- Last known Symphony state for status display.

Persist only user choices and lightweight session state.

## User Workflows

### Slash commands

- `/symphony:help`
  - Shows available commands, current attachment/process status, and examples.

- `/symphony:init [--force]`
  - Resolves the binary and runs `symphony init` in Pi's current working directory.

- `/symphony:doctor [workflow]`
  - Resolves the binary and runs `symphony doctor [workflow]`.

- `/symphony:start [workflow]`
  - Resolves the binary, starts `symphony [workflow] --no-tui`, attaches to the HTTP API, and opens the dashboard.

- `/symphony:attach <url>`
  - Attaches to an already-running Symphony server after verifying `GET /api/v1/state`.

- `/symphony:dashboard`
  - Opens the dashboard for the active attachment. If no server is attached, it offers to start Symphony.

- `/symphony:steer <ISSUE> <instruction>`
  - Sends an operator steer instruction to an active worker.

- `/symphony:refresh`
  - Requests an immediate Symphony poll.

- `/symphony:stop`
  - Stops only a Symphony process started by this extension.

### LLM tools

The extension registers tools that mirror operational actions:

- `symphony_init`
- `symphony_doctor`
- `symphony_start`
- `symphony_attach`
- `symphony_status`
- `symphony_steer`
- `symphony_refresh`
- `symphony_stop`

Tools return concise text plus structured details so the agent can reason about workers, process state, and steer outcomes.

## Dashboard Vertical Slices

The dashboard reaches full parity through vertical slices. Each slice ships an end-to-end workflow through resolver, process/client logic, UI, and tests.

This doc is the master design doc. We are using `/writing-plans` to create implementation plans for each vertical slice.

### Wave 1: foundation

#### Slice 1: start, attach, and health ✅

- Resolve and validate the Symphony binary.
- Start headless Symphony or attach to an existing server.
- Render connection status, project link, polling status, worker counts, and basic process ownership.
- Cover slash commands and tools for init, doctor, start, attach, status, stop, and help.

### Wave 2: worker operations

#### Slice 2: worker operations ✅

- Render the running workers table.
- Show selected-worker details: issue, tracker state, attempt, turn count, max turns, last activity, worker host, workspace path, and error preview.
- Support manual refresh.
- Support steering the selected worker from the dashboard.
- Show recent worker and runtime events.

 **Manual Run Instructions:**

 1. From repo root, launch Pi with the extension:
 pi -e ./apps/symphony/pi-extension
 Expected: Pi starts with Symphony commands available.
 2. In Pi, run /symphony:start .symphony/WORKFLOW.md or attach to an existing server with
 /symphony:attach <http://127.0.0.1>:<port>.
 Expected: Symphony attaches and dashboard/status shows the base URL.
 3. In the dashboard, press r.
 Expected: refresh is requested and worker counts update.
 4. Select a running worker with ↑ / ↓, press s, and enter an instruction.
 Expected: notification says Steer delivered to <ISSUE>.
 5. Press d to toggle details, then q to close.
 Expected: details toggle and dashboard exits without stopping Symphony.

### Wave 3: S03-S04 - retry, blocked, completed, and escalations ✅

#### Slice 3: retry, blocked, and completed issues

- Render retry queue, blocked issues, and completed issues.
- Add an issue detail panel that covers running, retry, blocked, and completed states.

#### Slice 4: escalations

- Render pending escalations.
- Support responding to escalations from the dashboard.
- Reflect escalation lifecycle events.

### Wave 4: S05-S06 - shared context and diagnostics

#### Slice 5: shared context

- Render shared context entries and summary.
- Support create and delete operations.
- Support scope filtering.

#### Slice 6: diagnostics parity

- Render rate limits, polling diagnostics, token totals, event stream counters, and event filters.
- Polish help text, key hints, empty states, and dashboard layout.

## Dashboard Interaction Model

- Arrow keys move selection.
- `r` refreshes state.
- `s` steers the selected worker when a worker is selected.
- `d` toggles selected item details.
- `e` opens or responds to the selected escalation after the escalation slice lands.
- `q` and Escape close the dashboard.

## Error Handling

- Missing binary: prompt once for an absolute path, validate it, then persist it.
- `init` and `doctor` failures: show exit code, stderr, and cwd.
- Start timeout: report command, cwd, expected API URL or port, captured output, and child status.
- Port conflicts: surface Symphony's startup failure. Do not silently retry ports unless Symphony reports a bound fallback port.
- Attach failure: distinguish unreachable URL, non-Symphony response, invalid JSON, and API error response.
- Steering failure: show Symphony API code and message, including `issue_not_running`, `no_active_session`, and `steer_failed`.
- Process ownership: `/symphony:stop` only terminates owned child processes.
- Shutdown: `stopOwnedOnShutdown` is configurable and defaults to `true`.

## Testing Strategy

- Unit tests for binary resolution, command argument parsing, API client error normalization, and dashboard state reducers.
- Mock HTTP server tests for state, refresh, steer, events, escalations, and shared context slices.
- Process manager tests using fake Symphony scripts for startup success, startup failure, port discovery, and stop behavior.
- TUI tests for dashboard rendering and key handling where practical.
- Package smoke test that validates the Pi package manifest and extension load behavior.

## Package and Distribution

`apps/symphony/pi-extension/package.json` declares:

- `name: "@kata-sh/pi-symphony-extension"`
- `keywords: ["pi-package"]`
- `pi.extensions` pointing at the extension entrypoint
- Pi core packages as peer dependencies
- Runtime libraries as production dependencies only

Installation examples should cover local path, git, and npm distribution once the package is published.

## Acceptance Criteria

- Users can initialize Symphony from Pi.
- Users can run Symphony doctor from Pi.
- Users can start Symphony headlessly from Pi and attach to the HTTP API.
- Users can attach to an already-running Symphony server.
- Users can open a Pi-native dashboard and monitor Symphony state.
- Users can steer running Symphony workers from commands, tools, and dashboard UI.
- The dashboard grows to full HTTP dashboard parity through vertical slices.
- The extension can be installed as a Pi package named `@kata-sh/pi-symphony-extension`.
