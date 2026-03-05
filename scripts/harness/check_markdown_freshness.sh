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
  if command -v rg >/dev/null 2>&1; then
    has_header=0
    rg -q "^Last reviewed: " "$file" || has_header=1
  else
    has_header=0
    grep -Eq "^Last reviewed: " "$file" || has_header=1
  fi

  if [[ "$has_header" -ne 0 ]]; then
    echo "Missing 'Last reviewed:' header in $file"
    failed=1
  fi
done

if [[ "$failed" -ne 0 ]]; then
  exit 1
fi

echo "Markdown freshness metadata check passed."
