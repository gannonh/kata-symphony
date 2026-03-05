# KAT-255 TypeScript Runtime Scaffold Design

## Context

- Ticket: KAT-255
- Goal: Bootstrap a TypeScript runtime scaffold and baseline toolchain that unblocks KAT-221 and KAT-252.
- Constraint: Keep scope scaffold-only (no tracker/orchestration behavior implementation in this ticket).

## Approved Decisions

- Package manager: `pnpm`
- Strictness level: Minimal strict (`TypeScript + Vitest + ESLint`; no formatter policy yet)
- `pnpm start` success behavior: log a single bootstrap confirmation line and exit `0`

## Options Considered

1. Minimal strict scaffold (selected)
   - Fast to ship, sufficient quality gates, low ceremony.
2. Monorepo-ready scaffold
   - Better long-term modularity but too much upfront complexity.
3. Ultra-minimal scaffold
   - Fastest bootstrap but weakens quality and adds near-term rework.

## Selected Approach

Adopt a single-package Node 22 + TypeScript scaffold with clear layer boundaries and TDD-first execution.

### File/Module Layout (scaffold only)

- `src/main.ts` (bootstrap entrypoint)
- `src/config/`
- `src/tracker/`
- `src/orchestrator/`
- `src/execution/`
- `src/observability/`
- `tests/`

### Tooling Surface

- `package.json` scripts:
  - `lint`
  - `typecheck`
  - `test`
  - `start`
  - `dev`
- TypeScript strict mode enabled
- Vitest configured for baseline tests
- ESLint configured for baseline linting

## TDD Plan

### Red

Create failing tests for:
1. Required script keys in `package.json`
2. Required scaffold directories in `src/`
3. Bootstrap behavior: `pnpm start` writes expected line and exits `0`

### Green

Implement minimum scaffold/config/entrypoint needed for all tests to pass.

### Refactor

Clean naming/structure while preserving passing tests.

## Verification Plan

Run:

1. `make check`
2. `pnpm run lint`
3. `pnpm run typecheck`
4. `pnpm test`
5. `pnpm start` (manual smoke)

## Scope Boundaries

### In Scope

- TS runtime/toolchain bootstrap
- No-op daemon entrypoint
- Layer-oriented module boundaries
- Local documentation updates for setup and commands

### Out of Scope

- Workflow loader/parser behavior (KAT-222/KAT-223)
- Core domain model implementation details (KAT-221)
- Tracker/workspace/agent execution logic
- Retry/reconciliation/state-machine behavior

## Risks and Mitigations

- Risk: Over-engineering scaffold before behavior exists.
  - Mitigation: Keep ticket limited to minimal strict scaffold and tests.
- Risk: Divergent tooling conventions later.
  - Mitigation: Keep scripts generic and update via dedicated tickets.

## Expected Documentation Changes

- `README.md` (pnpm setup and command usage)
- `PLANS.md` (execution progress/context)
- `AGENTS.md` only if workflow/tooling policy changes

## Handoff

After this design doc, transition to implementation planning (`writing-plans`) before code changes.
