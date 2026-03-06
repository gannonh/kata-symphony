#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/harness/common.sh
source "${ROOT_DIR}/scripts/harness/common.sh"

HARNESS_BASE_REF="${HARNESS_BASE_REF:-$(harness_resolve_base_ref || true)}"
export HARNESS_BASE_REF

echo "[ci-local] lint"
pnpm run lint

echo "[ci-local] typecheck"
pnpm run typecheck

echo "[ci-local] unit tests"
pnpm run test:unit

echo "[ci-local] integration tests"
pnpm run test:integration

echo "[ci-local] coverage"
pnpm run test:coverage

echo "[ci-local] doc relevance"
bash scripts/harness/check_doc_relevance.sh

echo "[ci-local] evidence contract"
bash scripts/harness/check_evidence_contract.sh

echo "[ci-local] decision links"
bash scripts/harness/check_decision_links.sh

echo "[ci-local] harness checks"
make check
