# kata-mono

pnpm monorepo (`pnpm@10.6.2`) with Turborepo orchestration. Rust app (Symphony) included.

## App Context

This mono-repo is comprised of the following primary applications:

- Kata Symphony: `apps/symphony` - @kata/symphony - Rust binary (Cargo scripts via package.json). Project-local Symphony config lives in `.symphony/`.
- Kata CLI: `apps/cli` - @kata-sh/cli - portable Kata Skills runtime and backend contract bridge

## Commands

```bash
pnpm install                     # Install all workspace dependencies
pnpm run validate                # Lint + typecheck + test (all packages, via Turborepo)
pnpm run validate:affected       # Same but only changed packages
pnpm run lint                    # ESLint across all packages
pnpm run typecheck               # TypeScript across all packages
pnpm run test                    # Test runner across all packages
pnpm run test:watch              # Watch mode
pnpm run test:coverage           # Coverage summary
pnpm run print:system-prompt     # Debug: print the agent system prompt
```

## Structure

```
apps/
├── cli/              # @kata-sh/cli - Kata Skills runtime and backend contract bridge
├── cli/skills-src/   # Source of truth for Kata Agent Skills
└── symphony/         # @kata/symphony - Rust binary (Cargo scripts via package.json)

packages/
├── core/             # Shared TypeScript types
├── shared/           # Shared business logic (agent, auth, config, MCP, channels, daemon)
├── ui/               # Shared React components (chat, markdown)
└── mermaid/          # Mermaid diagram renderer
```

## Turborepo

Tasks defined in `turbo.json`: `lint`, `typecheck` (topological), `test`, `build` (topological), `dev` (no cache).

```bash
turbo run typecheck --affected    # Only changed packages
turbo run lint typecheck test     # Full validation pipeline
```

Inputs include `.ts`, `.tsx`, `.js`, `.cjs`, `.mjs`, `.rs`, and `Cargo.toml` so both JS/TS and Rust changes invalidate the cache correctly.

## Testing

Turborepo orchestrates package-local test scripts via `turbo run test`.

| Package  | Runner / command | Notes                                                                                 |
| -------- | ---------------- | ------------------------------------------------------------------------------------- |
| cli      | `pnpm test`      | Vitest suite for CLI domain, backend adapters, skill bundle, and golden-path contract |
| symphony | `cargo test`     | Rust binary                                                                           |
| shared   | Vitest           | Package-local `vitest run`                                                            |

Pre-push hook runs `pnpm exec turbo run lint typecheck test --affected`, same command as CI.

## CI

`ci.yml` on pull_request to main:

- `validate`: `turbo run lint typecheck test --affected` (JS/TS + Rust via Turborepo)
- `gate`: aggregates results, sole required branch protection check

Release workflows trigger on push to main with path filters:
`cli-release.yml`, `symphony-release.yml`, `pi-symphony-extension-release.yml`

## Tech Stack

- **Runtime:** Bun (scripts, tests, subprocess execution)
- **UI:** React 18 + Vite + Tailwind CSS v4 + Radix UI
- **State:** Jotai atoms
- **AI:** @anthropic-ai/claude-agent-sdk + @anthropic-ai/sdk + @modelcontextprotocol/sdk
- **Build:** esbuild, Vite, and Turborepo
- **Rust:** Cargo (Symphony)

## Hard Rules

- Never use `git push --no-verify` or `git commit --no-verify`. If the gate fails, fix the problem.
- `git push --force` to main/master is forbidden( unless explcitly approved by an admin).

## Gotchas

- `.symphony/WORKFLOW.md` is the mono repo's active Symphony workflow. `symphony` without a workflow path resolves `.symphony/WORKFLOW.md` first, then `WORKFLOW.md`.
- Symphony prompt and hook paths are relative to the active `WORKFLOW.md` directory. Hooks receive `SYMPHONY_WORKSPACE_PATH` for workspace access.
- `CLAUDE.md` files in this repo are symlinks to `AGENTS.md`. Always edit `AGENTS.md`.
- Asset paths: use `getBundledAssetsDir(subfolder)` for bundled assets, never `import.meta.dir`.

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues for `gannonh/kata`; external PRs are not a triage surface. See `docs/agents/issue-tracker.md`.

### Triage labels

Triage uses the default five-label vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

Domain docs use a single-context layout. See `docs/agents/domain.md`.

## Open Knowledge Format docs

This repository maintains an [OKF](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md) bundle at `./docs`.

- Use `/okf read` when available, or read `./docs/index.md` directly before substantial work, to understand the current documentation map.
- Follow cross-links into relevant specs, ADRs, runbooks, guides, architecture notes, reference docs, and domain docs before changing related code.
- Keep `./docs/specs/index.md` current as the roadmap for active, planned, blocked, and completed work.
- Historical Superpowers plans/specs remain under `./docs/superpowers/`; the roadmap indexes them rather than relocating them.
- Add or update ADRs in `./docs/adrs` for durable architecture decisions.
- After substantial work, PRs, behavior changes, architecture decisions, migrations, or documentation moves, update the OKF bundle and add concise entries to the relevant `log.md` files.
- Maintain Markdown cross-links between related OKF concepts so future agents can traverse decisions, specs, architecture, runbooks, guides, and references.
- Every non-reserved Markdown file under `./docs` should have OKF frontmatter with at least a non-empty `type` field. `index.md` and `log.md` are reserved navigation/history files.
