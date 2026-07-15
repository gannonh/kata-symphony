---
type: Runbook
title: Kata Cli Manual Validation Runbook
description: Archived runbook document: Kata Cli Manual Validation Runbook.
tags: [archive, cli]
timestamp: 2026-07-15T20:00:00Z
---

# Kata CLI Phase A Manual Validation Runbook

Date: `2026-04-28`

## Goal

Validate the Phase A vertical slice with Pi as the harness and GitHub Projects v2 as the real backend.

Phase A is not accepted by unit tests alone. The final proof is a Pi session that runs the core Kata skill chain and leaves durable project, milestone, slice, task, and artifact state in GitHub.

## Scope

This runbook has two passes:

1. Monorepo validation using this repository as the project.
2. UAT validation using a separate todo-app repository with its own GitHub repo and GitHub Projects v2 project item state.

## Hard Constraints

- Run commands from the repository under test unless a step says otherwise.
- Use the CLI-owned skill source only.
- Local repo/dev skill source: `apps/cli/skills`.
- Published package skill source: bundled `skills/` inside `@kata-sh/cli`.
- Do not build or install skills from `apps/orchestrator-legacy`.
- GitHub backend means Projects v2 only.
- Do not use local markdown files as acceptance evidence for durable Kata state.

## Prerequisites

- Node 20+.
- `pnpm`.
- Pi coding agent installed and runnable as `pi`.
- GitHub token available as `GITHUB_TOKEN` or `GH_TOKEN`.
- `.kata/preferences.md` configured with:

```yaml
---
workflow:
  mode: github
github:
  repoOwner: <owner>
  repoName: <repo>
  stateMode: projects_v2
  githubProjectNumber: <project-number>
---
```

## Build From Monorepo Root

Run from this repo:

```bash
pnpm --dir apps/cli run build
```

Expected:

- `apps/cli/dist/loader.js` exists.
- `apps/cli/skills` contains exactly the Phase A skill bundle.
- `apps/cli/skills/kata-new-milestone/SKILL.md` exists.
- `apps/cli/skills/kata-complete-milestone/SKILL.md` exists.
- `apps/cli/skills/kata-discuss-phase` does not exist.
- `apps/cli/skills/kata-quick` does not exist.

## Pass 1: Monorepo Project

### 1. Install Skills Into Pi

Run from the monorepo root:

```bash
node apps/cli/dist/loader.js setup --pi
```

Expected:

- JSON output has `"ok": true`.
- `mode` is `"pi-install"`.
- `pi.skillsSourceResolution` is `"cli-workspace"`.
- `pi.skillsSourceDir` points to `apps/cli/skills`.
- `~/.pi/agent/skills/kata-health/SKILL.md` exists.
- `~/.pi/agent/skills/kata-new-milestone/SKILL.md` exists.
- `~/.pi/agent/skills/kata-complete-milestone/SKILL.md` exists.
- `~/.pi/agent/skills/kata-discuss-phase` does not exist.
- `~/.pi/agent/skills/kata-quick` does not exist.

### 2. Run Doctor

Run from the monorepo root:

```bash
node apps/cli/dist/loader.js doctor
```

Expected:

- `skills-source` is `ok`.
- `pi-skills-dir` is `ok`.
- `pi-settings` is `ok`.
- `backend-config` is `ok` and reports `github projects_v2`.
- `github-token` is not `invalid`.

Note: `github-token` may currently be `warn` because doctor checks token presence but does not yet perform live Project v2 field validation. Real backend operations below are the acceptance proof.

### 3. Smoke Test Real Backend Contract

Create a temporary input:

```bash
cat > /tmp/kata-health-check.json <<'JSON'
{}
JSON
```

Run:

```bash
node apps/cli/dist/loader.js call health.check --input /tmp/kata-health-check.json
```

Expected:

- Response has `"ok": true`.
- Response includes GitHub backend health data.
- Any failure is investigated before launching Pi.

### 4. Run Phase A Skill Chain In Pi

Start Pi from the monorepo root:

```bash
pi
```

Run these skills in order:

```text
/skill:kata-setup
/skill:kata-new-project
/skill:kata-new-milestone
/skill:kata-plan-phase
/skill:kata-execute-phase
/skill:kata-verify-work
/skill:kata-complete-milestone
/skill:kata-new-milestone
/skill:kata-plan-phase
```

Expected:

- Each skill is discoverable by Pi.
- Each skill uses `@kata-sh/cli` for durable backend IO.
- The workflow does not ask for legacy `.planning` files.
- The workflow does not reference `kata-tools.cjs`.
- Discussion is integrated into the active workflow, not routed to a standalone discuss skill.

### 5. Capture Monorepo Evidence

Record these URLs or IDs:

```text
GitHub Project URL:
Project tracking issue URL:
First milestone issue URL:
First slice issue URL:
First task issue URL:
Plan artifact comment URL:
Execution summary artifact comment URL:
UAT or verification artifact comment URL:
Completed milestone evidence URL:
Second milestone issue URL:
Second plan artifact comment URL:
Pi transcript location:
```

## Pass 2: UAT Todo App Project

### 1. Create UAT Repository

Run outside the monorepo:

```bash
mkdir -p ~/kata-cli-uat-todo
cd ~/kata-cli-uat-todo
pnpm create vite@latest . --template react-ts
pnpm install
git init
git add .
git commit -m "chore: initialize uat todo app"
gh repo create kata-cli-uat-todo --private --source=. --remote=origin --push
```

### 2. Configure UAT Backend

Create `~/kata-cli-uat-todo/.kata/preferences.md`:

```yaml
---
workflow:
  mode: github
github:
  repoOwner: <owner>
  repoName: kata-cli-uat-todo
  stateMode: projects_v2
  githubProjectNumber: <project-number>
---
```

Use a GitHub Projects v2 project that the token can read and mutate.

### 3. Validate CLI From UAT Repo

Run from `~/kata-cli-uat-todo`:

```bash
node /Users/gannonhall/.codex/worktrees/edf7/kata-mono/apps/cli/dist/loader.js doctor
```

Expected:

- `backend-config` is `ok`.
- `github-token` is not `invalid`.

### 4. Run UAT Skill Chain In Pi

Run from `~/kata-cli-uat-todo`:

```bash
pi
```

Run:

```text
/skill:kata-setup
/skill:kata-new-project
/skill:kata-new-milestone
/skill:kata-plan-phase
/skill:kata-execute-phase
/skill:kata-verify-work
```

Expected:

- The todo app repo receives durable GitHub-backed Kata state.
- The work creates or updates real GitHub issues/project items.
- Artifacts are written as GitHub issue comments.
- The workflow can be evaluated independently from the monorepo.

### 5. Capture UAT Evidence

Record:

```text
UAT repo URL:
GitHub Project URL:
Project tracking issue URL:
Milestone issue URL:
Slice issue URL:
Task issue URL:
Plan artifact comment URL:
Verification artifact comment URL:
Pi transcript location:
```

## Acceptance Decision

Phase A is accepted only when:

1. `pnpm --dir apps/cli run build` passes.
2. `node apps/cli/dist/loader.js setup --pi` installs the nine Phase A skills into Pi.
3. `node apps/cli/dist/loader.js doctor` is not `invalid`.
4. The monorepo Pi skill chain completes against real GitHub Projects v2.
5. The UAT todo-app Pi skill chain completes against real GitHub Projects v2.
6. Evidence URLs are recorded for both passes.

If any step fails, stop and create a focused fix before continuing the acceptance chain.
