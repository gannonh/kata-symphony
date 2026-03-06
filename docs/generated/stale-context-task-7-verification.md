# Stale Context Task 7 Verification

Last reviewed: 2026-03-06

## Commands

- `pnpm vitest tests/harness/check-stale-context.test.ts`
- `bash scripts/harness/check_stale_context.sh`

## Expected Result

The fixture test should pass and the live repo audit should print a readable
three-section report for stale subsystem candidates, orphaned design docs, and
missing evidence links.
