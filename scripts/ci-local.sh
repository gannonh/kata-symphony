#!/usr/bin/env bash
set -euo pipefail

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
