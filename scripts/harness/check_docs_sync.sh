#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "Not in a git repository; skipping docs sync check."
  exit 0
fi

BASE_REF="${1:-}"
if [[ -z "$BASE_REF" ]]; then
  if [[ -n "${GITHUB_BASE_REF:-}" ]]; then
    BASE_REF="origin/${GITHUB_BASE_REF}"
    git fetch --no-tags --depth=1 origin "${GITHUB_BASE_REF}" >/dev/null 2>&1 || true
  else
    BASE_REF="$(git rev-parse --verify HEAD~1 2>/dev/null || true)"
  fi
fi

if [[ -n "$BASE_REF" ]]; then
  changed_files="$(git diff --name-only "${BASE_REF}"...HEAD)"
else
  if git rev-parse --verify HEAD >/dev/null 2>&1; then
    changed_files="$(printf '%s\n' \
      "$(git diff --name-only HEAD)" \
      "$(git diff --name-only --cached)" \
      "$(git ls-files --others --exclude-standard)" | sed '/^$/d' | sort -u)"
  else
    echo "Could not determine base ref and no HEAD exists; skipping docs sync check."
    exit 0
  fi
fi

if [[ -z "${changed_files:-}" ]]; then
  echo "No changed files; docs sync check passed."
  exit 0
fi

code_changed=0
docs_changed=0

while IFS= read -r file; do
  [[ -z "$file" ]] && continue

  case "$file" in
    docs/*|AGENTS.md|ARCHITECTURE.md|PLANS.md|QUALITY_SCORE.md|RELIABILITY.md|SECURITY.md|README.md)
      docs_changed=1
      ;;
  esac

  case "$file" in
    src/*|services/*|packages/*|app/*|internal/*|cmd/*|SPEC.md|WORKFLOW.md|*.ts|*.tsx|*.js|*.jsx|*.py|*.go|*.rs)
      code_changed=1
      ;;
  esac
done <<< "$changed_files"

if [[ "$code_changed" -eq 1 && "$docs_changed" -eq 0 ]]; then
  echo "Code/spec/workflow changes detected without documentation updates."
  echo "Update at least one of: docs/*, AGENTS.md, ARCHITECTURE.md, PLANS.md, QUALITY_SCORE.md, RELIABILITY.md, SECURITY.md, README.md"
  exit 1
fi

echo "Docs sync check passed."
