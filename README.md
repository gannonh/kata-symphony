# kata-symphony

Symphony specification and implementation workspace.

## Local Setup

```bash
pnpm install
```

## Required Environment

Copy `.env.example` and set:

- `LINEAR_API_KEY`
- `SYMPHONY_WORKSPACE_ROOT` (where per-issue workspaces are created)

## Harness Checks

Run all checks:

```bash
make check
```

Run only lint:

```bash
make lint
pnpm run lint
```

Run only harness docs/contract checks:

```bash
make harness
```

Run TypeScript checks:

```bash
pnpm run typecheck
```

Run tests:

```bash
pnpm test
```

Run unit and integration tests separately:

```bash
pnpm run test:unit
pnpm run test:integration
```

Run coverage gate (100% threshold on `src/**/*.ts`):

```bash
pnpm run test:coverage
```

Run local CI (same lint/typecheck/test/coverage gates as CI):

```bash
pnpm run ci:local
```

Run bootstrap entrypoint:

```bash
pnpm start
```

## Git Hook Gate

Install repository hooks and enforce local CI before push:

```bash
pnpm run hooks:install
```

This configures `core.hooksPath` to `.githooks`, including a `pre-push` hook that runs:

- `pnpm run ci:local`

## What gets enforced

1. `scripts/harness/lint_repo.sh`
   - shell script syntax checks
   - optional `shellcheck`
   - optional JS/TS lint if `package.json` + lint script exist
2. `scripts/harness/check_repo_contract.sh`
   - required repository files/directories exist
3. `scripts/harness/check_markdown_freshness.sh`
   - key docs include `Last reviewed:` metadata
4. `scripts/harness/check_docs_sync.sh`
   - fails when code/spec/workflow changes are made without updating docs

## CI

GitHub Actions workflow:

- `.github/workflows/harness-engineering.yml`

Runs on pull requests and pushes to `main`.
