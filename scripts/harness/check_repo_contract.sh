#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

required_files=(
  "AGENTS.md"
  "ARCHITECTURE.md"
  "PLANS.md"
  "QUALITY_SCORE.md"
  "RELIABILITY.md"
  "SECURITY.md"
  "WORKFLOW.md"
  "docs/README.md"
  "docs/design-docs/index.md"
  "docs/exec-plans/README.md"
  "docs/generated/README.md"
  "docs/product-specs/index.md"
  "docs/references/harness-engineering.md"
  "docs/harness/BUILDING-WITH-HARNESS.md"
  "docs/harness/context-map.yaml"
  "docs/harness/change-evidence-schema.md"
  "docs/generated/change-evidence/.gitkeep"
)

missing=0
for file in "${required_files[@]}"; do
  if [[ ! -f "$file" ]]; then
    echo "Missing required file: $file"
    missing=1
  fi
done

required_dirs=(
  "docs/exec-plans/active"
  "docs/exec-plans/completed"
  "docs/generated/change-evidence"
  "scripts/harness"
)

for dir in "${required_dirs[@]}"; do
  if [[ ! -d "$dir" ]]; then
    echo "Missing required directory: $dir"
    missing=1
  fi
done

if [[ "$missing" -ne 0 ]]; then
  exit 1
fi

echo "Repository contract check passed."
