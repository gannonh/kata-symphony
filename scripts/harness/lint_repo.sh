#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

echo "[lint] Checking shell script syntax..."
while IFS= read -r script; do
  bash -n "$script"
done < <(find scripts -type f -name '*.sh' | sort)

if command -v shellcheck >/dev/null 2>&1; then
  echo "[lint] Running shellcheck..."
  shellcheck $(find scripts -type f -name '*.sh' | sort)
else
  echo "[lint] shellcheck not installed; skipping semantic shell lint."
fi

if [[ -f package.json ]]; then
  echo "[lint] package.json detected."
  if command -v pnpm >/dev/null 2>&1; then
    if pnpm run -r --if-present lint >/dev/null 2>&1; then
      pnpm run -r --if-present lint
    else
      echo "[lint] No pnpm lint script defined; skipping JS/TS lint."
    fi
  elif command -v npm >/dev/null 2>&1; then
    if npm run --if-present lint >/dev/null 2>&1; then
      npm run --if-present lint
    else
      echo "[lint] No npm lint script defined; skipping JS/TS lint."
    fi
  else
    echo "[lint] Node package detected but pnpm/npm unavailable."
    exit 1
  fi
else
  echo "[lint] No package.json found; skipping JS/TS lint."
fi

echo "[lint] Repository lint passed."

