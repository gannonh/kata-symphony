#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

targets=(
  "ARCHITECTURE.md"
  "PLANS.md"
  "QUALITY_SCORE.md"
  "RELIABILITY.md"
  "SECURITY.md"
  "docs/README.md"
  "docs/design-docs/index.md"
  "docs/exec-plans/README.md"
  "docs/generated/README.md"
  "docs/product-specs/index.md"
  "docs/references/harness-engineering.md"
  "docs/harness/BUILDING-WITH-HARNESS.md"
)

failed=0
for file in "${targets[@]}"; do
  if ! rg -q "^Last reviewed: " "$file"; then
    echo "Missing 'Last reviewed:' header in $file"
    failed=1
  fi
done

if [[ "$failed" -ne 0 ]]; then
  exit 1
fi

echo "Markdown freshness metadata check passed."

