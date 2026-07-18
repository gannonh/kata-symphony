<div align="center">

# Kata

**型** · /ˈkɑːtɑː/ · *noun* - a choreographed pattern practiced repeatedly until perfected

![alt text](assets/brand/logo-circle-dark.png)
<br>
[kata.sh](https://kata.sh)

</div>

---

## Kata Symphony

**Kata Symphony** is a software factory agent orchestrator (and companion Pi coding agent extension). It polls GitHub or Linear projects for candidate tickets and dispatches parallel agent sessions to work on them autonomously — managing the full ticket lifecycle from backlog through implementation, PR creation, automated code review, human review, and merge, with multi-turn sessions and real-time event streaming. Symphony can be run headlessly, using the native Linear or GitHub project boards as the primary UI surface, or steered and monitored with the integrated web, Rust TUI, or Pi extension dashboard.

| Piece | Path | Role |
| --- | --- | --- |
| [Symphony binary](apps/symphony/README.md) | `apps/symphony` | Orchestrator: polls GitHub or Linear, dispatches parallel agent sessions, manages the full PR lifecycle |
| [Pi Symphony extension](apps/symphony/pi-extension/README.md) | `apps/symphony/pi-extension` | Primary operator UX inside Pi (`/symphony:*`, live console) |

**[Kata CLI](apps/cli/README.md)** is a secondary, opinionated planning/backend workflow that integrates cleanly with Symphony.

Quick start:

```bash
cd apps/symphony
cargo build --release

cd /path/to/your/repo
/path/to/kata/apps/symphony/target/release/symphony init
$EDITOR .symphony/WORKFLOW.md
$EDITOR .symphony/prompts/repo.md

GH_TOKEN=github_pat_... /path/to/kata/apps/symphony/target/release/symphony --port 8080
# or: LINEAR_API_KEY=lin_api_... /path/to/kata/apps/symphony/target/release/symphony --port 8080
```

Key features:

- **GitHub and Linear integration** - polls for issues, manages state transitions, respects priorities and dependency graphs
- **Parallel agents** - configurable concurrency with per-state slot limits
- **Multi-turn sessions** - agents continue on the same worker session across turns, preserving conversation history
- **Full PR lifecycle** - agents create PRs, address review feedback, resolve comment threads, and merge
- **Real-time streaming** - events flow from workers to the orchestrator as they happen
- **Project-local config** - `symphony init` writes `.symphony/WORKFLOW.md`, starter prompts, and reference docs
- **Dynamic config reload** - WORKFLOW.md changes take effect without restart
- **SSH worker pools** - distribute sessions across remote machines
- **HTTP dashboard + JSON API** - live observability at `localhost:8080`

Use Kata Symphony when you want:

- autonomous ticket-to-merge execution without human intervention
- parallel agent sessions working through a tracker backlog
- a headless orchestrator you can run on a server or CI

### Pi Coding Agent extension

The recommended way to operate Symphony day to day is the Pi extension. Install `@kata-sh/pi-symphony-extension` to initialize, start, attach to, and monitor Symphony from Pi. It adds `/symphony:*` commands and a live console for running workers, retry queue entries, blocked issues, completed issues, and pending escalations.

Symphony binary and Pi extension releases are **coupled**: they ship together at the same version.

```bash
pi install npm:@kata-sh/pi-symphony-extension
# or, from the monorepo package:
pi install git:github.com/gannonh/kata
```

Read more:

- [apps/symphony/README.md](apps/symphony/README.md)
- [apps/symphony/pi-extension/README.md](apps/symphony/pi-extension/README.md)

## Kata CLI

Kata CLI is a secondary product: the backend IO and setup CLI for Kata Skills. It owns typed project, milestone, slice, task, and artifact operations while agent harnesses such as Pi and Symphony own chat and worker execution. It is especially useful as an opinionated planning workflow that integrates nicely with Symphony.

Quick start:

```bash
npm install -g @kata-sh/cli
kata setup --pi
```

For local development:

```bash
pnpm --dir apps/cli run build
pnpm --dir apps/cli run test
```

Use Kata CLI when you want:

- portable Kata Skills installable into multiple harnesses
- durable backend operations through GitHub Projects v2 or Linear
- typed backend operations for planning, execution, verification, progress, and milestone completion
- a structured planning layer that feeds work into Symphony

Read more in [apps/cli/README.md](apps/cli/README.md).

## Repository layout

| Path | Purpose |
| --- | --- |
| `apps/symphony` | Kata Symphony (Rust binary) |
| `apps/symphony/pi-extension` | Pi Coding Agent extension for Symphony |
| `apps/cli` | Kata CLI (secondary) |
| `apps/cli/skills-src` | Source of truth for Kata Agent Skills |
| `packages/core` | Shared types |
| `packages/shared` | Shared agent, auth, config, git, session, and source logic |
| `packages/ui` | Shared UI components |
| `packages/mermaid` | Mermaid diagram renderer |

## Local Development

Install dependencies:

```bash
pnpm install
```

Install repo-managed git hooks:

```bash
pnpm run githooks:install
```

Common commands:

| Command | Purpose |
| --- | --- |
| `pnpm run validate` | Lint + typecheck + test all packages via Turbo |
| `pnpm run validate:affected` | Same, only changed packages |
| `cd apps/symphony && cargo build` | Build Kata Symphony |

## Testing

All testing is orchestrated by Turborepo. Each package owns its test runner.

| Command | Runs |
| --- | --- |
| `pnpm run test` | All package tests via Turborepo |
| `pnpm run test:affected` | Only changed packages |

| Package | Test runner | Notes |
| --- | --- | --- |
| symphony | cargo test | Rust, runs through package.json shim |
| all others | package-local scripts | JS/TS packages run through Turborepo |

A pre-push git hook runs `turbo run lint typecheck test --affected` before every push.

## Releases

Releases are manually dispatched (Symphony also has a scheduled nightly):

| Workflow | Ships |
| --- | --- |
| `symphony-release.yml` | Symphony binary + Pi extension together (same version) |
| `cli-release.yml` | CLI only (independent) |

See `.agents/skills/releasing-kata/SKILL.md` for the full release process.

## License

Kata packages use the licenses declared in their package directories.
