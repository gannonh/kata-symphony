#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

run_segment() {
  local segment="$1"
  shift

  echo "::group::${segment}"
  if "$@"; then
    echo "✅ ${segment} passed"
    echo "::endgroup::"
    return 0
  else
    local exit_code=$?
    echo "::error title=GitHub backend validation failed::${segment} failed (exit ${exit_code})."
    echo "::error::Review the grouped output above to see the exact backend contract that regressed."
    echo "::endgroup::"
    return "$exit_code"
  fi
}

echo "Running deterministic GitHub backend validation lane"
echo "Negative-path signal checks included in this lane:"
echo "- CLI: setup/source policy + golden-path runtime contract checks"
echo "- Symphony: unknown Project v2 status actionable error"

run_segment \
  "CLI GitHub backend contract suites (Vitest)" \
  pnpm --dir apps/cli exec vitest run \
  src/tests/setup-source.vitest.test.ts \
  src/tests/golden-path.pi-github.vitest.test.ts

run_segment \
  "Symphony GitHub backend execution contracts (cargo test)" \
  pnpm --dir apps/symphony exec cargo test \
  --test github_adapter_tests \
  --test github_execution_contract_tests

UAT_EVIDENCE_TEST=.agents/skills/uat-evidence/scripts/symphony-runtime-config.test.mjs
if [ -f "$UAT_EVIDENCE_TEST" ]; then
  run_segment \
    "Symphony UAT evidence cleanup contracts (node test)" \
    node --test \
    "$UAT_EVIDENCE_TEST"
else
  echo "::group::Symphony UAT evidence cleanup contracts (node test)"
  echo "⚠️  Skipped: the uat-evidence skill test file is not installed in this checkout ($UAT_EVIDENCE_TEST)."
  echo "    Install the uat-evidence skill locally and rerun this lane to exercise the contracts."
  echo "::endgroup::"
fi

echo "GitHub backend validation lane completed successfully."
