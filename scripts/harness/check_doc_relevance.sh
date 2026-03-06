#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT_DIR"

canonical_docs=(
  "AGENTS.md"
  "ARCHITECTURE.md"
  "PLANS.md"
  "QUALITY_SCORE.md"
  "RELIABILITY.md"
  "SECURITY.md"
  "SPEC.md"
  "WORKFLOW.md"
  "docs/harness/BUILDING-WITH-HARNESS.md"
  "docs/harness/change-evidence-schema.md"
  "docs/harness/context-map.yaml"
  "docs/references/harness-engineering.md"
)

BASE_REF="${1:-}"
if [[ -z "$BASE_REF" ]]; then
  if [[ -n "${GITHUB_BASE_REF:-}" ]]; then
    BASE_REF="origin/${GITHUB_BASE_REF}"
    git fetch --no-tags --depth=1 origin "${GITHUB_BASE_REF}" >/dev/null 2>&1 || true
  else
    BASE_REF="$(git rev-parse --verify HEAD~1 2>/dev/null || true)"
  fi
fi

if [[ -z "$BASE_REF" ]]; then
  echo "Could not determine base ref; skipping doc relevance check."
  exit 0
fi

changed_files="$(git diff --name-only "${BASE_REF}"...HEAD)"

if [[ -z "${changed_files:-}" ]]; then
  echo "No changed files; doc relevance check passed."
  exit 0
fi

find_evidence_path() {
  if [[ -n "${HARNESS_EVIDENCE_PATH:-}" ]]; then
    printf '%s\n' "${HARNESS_EVIDENCE_PATH}"
    return 0
  fi

  local latest
  latest="$(find docs/generated/change-evidence -maxdepth 1 -type f -name '*.json' | sort | tail -n 1)"
  if [[ -n "$latest" ]]; then
    printf '%s\n' "$latest"
  fi
}

evidence_path="$(find_evidence_path || true)"
allowed_docs=""
if [[ -n "${evidence_path:-}" && -f "${evidence_path}" ]]; then
  allowed_docs="$(node - "${evidence_path}" <<'EOF'
const fs = require('node:fs')
const file = process.argv[2]
const parsed = JSON.parse(fs.readFileSync(file, 'utf8'))
const combined = [
  ...(Array.isArray(parsed.contextLoaded) ? parsed.contextLoaded : []),
  ...(Array.isArray(parsed.canonicalDocsUpdated) ? parsed.canonicalDocsUpdated : []),
  ...((Array.isArray(parsed.waivers) ? parsed.waivers : []).flatMap((entry) =>
    entry && typeof entry.doc === 'string' ? [entry.doc] : [],
  )),
]
const unique = [...new Set(combined)].sort((a, b) => a.localeCompare(b))
process.stdout.write(`${unique.join('\n')}\n`)
EOF
)"
fi

contains_line() {
  local needle="$1"
  while IFS= read -r item; do
    [[ -z "$item" ]] && continue
    if [[ "$item" == "$needle" ]]; then
      return 0
    fi
  done <<< "$2"

  return 1
}

is_metadata_only_diff() {
  local target="$1"
  local before_content
  local after_content

  before_content="$(git show "${BASE_REF}:${target}" 2>/dev/null || true)"
  after_content="$(cat "$target")"

  local normalized_before
  local normalized_after
  normalized_before="$(printf '%s' "$before_content" | sed -E '/^Last reviewed: /d; s/[[:space:]]+$//')"
  normalized_after="$(printf '%s' "$after_content" | sed -E '/^Last reviewed: /d; s/[[:space:]]+$//')"

  [[ "$normalized_before" == "$normalized_after" ]]
}

failed=0
while IFS= read -r file; do
  [[ -z "$file" ]] && continue

  if ! contains_line "$file" "$(printf '%s\n' "${canonical_docs[@]}")"; then
    continue
  fi

  if is_metadata_only_diff "$file"; then
    echo "Canonical doc change is metadata-only: $file"
    failed=1
    continue
  fi

  if [[ -n "$allowed_docs" ]] && ! contains_line "$file" "$allowed_docs"; then
    echo "Canonical doc changed outside evidence-declared context: $file"
    failed=1
  fi
done <<< "$changed_files"

if [[ "$failed" -ne 0 ]]; then
  exit 1
fi

echo "Doc relevance check passed."
