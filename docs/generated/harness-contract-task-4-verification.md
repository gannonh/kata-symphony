# Harness Contract Task 4 Verification

Last reviewed: 2026-03-06

## Commands

- `pnpm vitest tests/harness/check-evidence-contract.test.ts tests/harness/generate-change-evidence.test.ts`
- `bash scripts/ci-local.sh`

## Expected Result

Both commands pass, proving that the stricter evidence contract and decision-link
checks work together with the existing local CI path.
