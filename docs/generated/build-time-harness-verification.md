# Build-Time Harness Verification

Last reviewed: 2026-03-06

## Commands

- `pnpm run lint`
- `pnpm run typecheck`
- `pnpm test`
- `bash scripts/ci-local.sh`
- `make check`
- `bash scripts/harness/check_stale_context.sh`

## Results

- All commands passed on `8b86ec6651e348b48bb6453c8c4a8ecbab0600a8`.
- `pnpm test` passed with `50` test files and `208` tests passing.
- `bash scripts/ci-local.sh` passed, including the doc relevance, evidence
  contract, decision-link, repo-contract, freshness, and docs-sync checks.
- `make check` passed after the rollout changes.
- Review-fix targeted harness tests now pass locally:
  - `pnpm vitest tests/harness/check-doc-relevance.test.ts`
  - `pnpm vitest tests/harness/check-evidence-contract.test.ts`
- The final repair batch also passes `bash scripts/ci-local.sh` after the
  evidence artifact was brought back into sync with the branch diff.

## Residual Risks

- `check_stale_context.sh` currently reports `src/execution/**` and
  `scripts/harness/**` as stale-context candidates. That is expected for the
  first rollout because the context map remains intentionally conservative and
  the audit is informational rather than gating.
- The task-by-task evidence model is now in place, but the rollout still
  depends on agents following the new skill guidance consistently during future
  multi-file changes.
