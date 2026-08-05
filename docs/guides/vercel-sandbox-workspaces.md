---
type: Guide
title: Vercel Sandbox workspaces for Orca
description: Provision disposable Orca workspaces from a reusable, Codex-authenticated Vercel Sandbox snapshot.
tags: [orca, vercel, sandbox, codex]
timestamp: 2026-08-05T16:03:26Z
---

# Overview

The `vercel-sandbox` environment recipe in `orca.yaml` creates one disposable Vercel Sandbox per Orca workspace. It restores a prebuilt snapshot, updates the checkout to `main`, starts `orca serve` on port `7331`, and returns the pairing result to Orca.

The setup uses two reusable layers:

1. A base snapshot containing the repository, release builds, Node 24, pnpm, Rust, Codex, and Orca.
2. An authenticated snapshot containing the Codex device login.

Non-secret provider and snapshot wiring lives in `scripts/orca-vm/vercel-sandbox-state.json`. Provider, Git, and agent credentials are never written there.

# Prerequisites

- Orca `v1.4.168` or a deliberately tested replacement.
- Vercel CLI authenticated for the configured scope and project.
- A Vercel plan that supports the configured 60-minute sandbox timeout and 4-vCPU runtime.
- `VERCEL_TOKEN` and `GH_TOKEN` stored in the repository's ignored `.env` file or exported in the calling environment. Lifecycle scripts load missing credentials from `.env` because the Orca desktop app does not inherit terminal exports.
- A Codex account available for the one-time device-auth step.

Provisioning and live validation create billable Vercel resources. Obtain approval before running them.

# Build or refresh the snapshots

The scripts automatically load missing credentials from the ignored repository `.env`. Exported values take precedence. For direct `vercel` commands outside these scripts, load credentials without printing them:

```bash
set -a
. ./.env
set +a
```

Build the base snapshot:

```bash
scripts/orca-vm/vercel-sandbox-base-snapshot.sh
```

Start the authentication sandbox:

```bash
scripts/orca-vm/vercel-sandbox-base-auth.sh start
```

Run the exact interactive `vercel sandbox exec --interactive --tty ...` command printed by that script. Complete `codex login --device-auth`, then seal the authenticated snapshot:

```bash
scripts/orca-vm/vercel-sandbox-base-auth.sh finish
```

Snapshots expire after 30 days. Repeat both phases when the active snapshot expires, Codex authentication stops working, or the pinned toolchain changes.

# Validate

Run the free static check first:

```bash
orca vm recipe doctor vercel-sandbox --repo-path "$PWD" --json
```

With explicit approval for billable provisioning, run the live create/validate/destroy check:

```bash
orca vm recipe doctor vercel-sandbox --repo-path "$PWD" --provision --json
```

A successful live check reports passing `recipe.provision`, `recipe.result.project_root`, and `recipe.destroy.run` checks. The destroy phase must remove the test sandbox.

# Runtime behavior

- `vercel-sandbox-create.sh` restores the authenticated snapshot and publishes port `7331`.
- `vercel-sandbox-start.sh` fetches `main`, rebuilds only when `.orca-built` does not match the checked-out commit, and starts `orca serve`.
- The Linux Orca launcher uses the packaged CLI through `ELECTRON_RUN_AS_NODE=1`.
- A persistent Xvfb display supports the Electron runtime. The launcher validates its PID and replaces stale X11 sockets restored from snapshots.
- Suspend, resume, and destroy read the resource identifier from Orca's lifecycle payload; resume returns a fresh pairing result.
- Failure traps remove partially created sandboxes.

The recipe appears in Orca's workspace picker only after `orca.yaml` is committed and available from the project's primary checkout. Static and live doctor checks can run from an uncommitted working copy.

# Troubleshooting

- **Missing `VERCEL_TOKEN`**: add it to the repository's ignored `.env` file. Orca desktop processes do not inherit variables exported in an unrelated terminal.
- **Missing `snapshotId`**: run the base and auth phases in order.
- **Codex unauthenticated**: start a new auth sandbox and repeat device auth; do not copy the host `~/.codex` directory.
- **Electron zygote or display failure**: confirm the snapshot contains `/usr/local/bin/orca`, Xvfb, and the Amazon Linux Electron dependencies installed by the base script.
- **Stale X11 socket**: the launcher should remove it automatically when its recorded Xvfb PID is absent. Rebuild the base snapshot if the launcher predates that check.
- **Remote command looks successful but no marker appears**: Vercel CLI can return without propagating a remote failure. The scripts require explicit success markers and print the captured remote output.
- **Unknown or expired snapshot**: rebuild and reauthenticate; the state file is updated automatically.

# Security

Keep `VERCEL_TOKEN`, `GH_TOKEN`, pairing codes, and Codex credentials out of version control. Snapshot IDs, provider scope/project identifiers, and resource names are non-secret lifecycle state. The Git token is passed only as an ephemeral provider environment variable and used through a temporary `GIT_ASKPASS` helper.
