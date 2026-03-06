#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="${REPO_ROOT:-$(cd "${SCRIPT_DIR}/../.." && pwd)}"
cd "$ROOT_DIR"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/common.sh"

BASE_REF="${1:-}"
if [[ -z "$BASE_REF" ]]; then
  BASE_REF="$(harness_resolve_base_ref || true)"
fi

changed_files="$(harness_collect_changed_files 0 "$BASE_REF")"
if [[ -z "${changed_files:-}" ]]; then
  echo "No changed files; decision link check passed."
  exit 0
fi

evidence_path=""
if evidence_path="$(harness_find_matching_evidence_path "$changed_files" 2>/dev/null)"; then
  harness_assert_evidence_matches_changed_files "$evidence_path" "$changed_files"
fi

if [[ -z "${evidence_path:-}" || ! -f "${evidence_path}" ]]; then
  echo "No matching evidence artifact found; skipping decision link checks."
  exit 0
fi

markdown_path="${evidence_path%.json}.md"
if [[ ! -f "$markdown_path" ]]; then
  echo "Missing markdown evidence artifact: $markdown_path"
  exit 1
fi

validation_output="$(node - "${evidence_path}" "${markdown_path}" "${ROOT_DIR}" <<'EOF'
const fs = require('node:fs')
const path = require('node:path')
const YAML = require('yaml')

const evidencePath = process.argv[2]
const markdownPath = process.argv[3]
const rootDir = process.argv[4]
const parsed = JSON.parse(fs.readFileSync(evidencePath, 'utf8'))
const markdown = fs.readFileSync(markdownPath, 'utf8')
const contextMap = YAML.parse(fs.readFileSync(path.join(rootDir, 'docs/harness/context-map.yaml'), 'utf8'))
const rules = Array.isArray(contextMap?.rules) ? contextMap.rules : []
const errors = []

const fileExists = (relativePath) => fs.existsSync(path.join(rootDir, relativePath))

for (const field of ['decisionArtifacts', 'verificationArtifacts', 'canonicalDocsUpdated']) {
  const values = Array.isArray(parsed[field]) ? parsed[field] : []
  for (const item of values) {
    if (typeof item !== 'string' || item.trim().length === 0) {
      errors.push(`${field} contains an empty path.`)
      continue
    }
    if (!fileExists(item)) {
      errors.push(`Linked artifact does not exist: ${item}`)
    }
    if (!markdown.includes(item)) {
      errors.push(`Markdown evidence is missing linked path: ${item}`)
    }
  }
}

const globToRegExp = (pattern) => new RegExp(`^${pattern
  .replace(/[.+^${}()|[\]\\]/g, '\\$&')
  .replace(/\*\*/g, '::DOUBLE_STAR::')
  .replace(/\*/g, '[^/]*')
  .replace(/::DOUBLE_STAR::/g, '.*')}$`)

const changedFiles = Array.isArray(parsed.changedFiles) ? parsed.changedFiles : []
const allowedDocs = new Set()
for (const rule of rules) {
  const matcher = globToRegExp(rule.pattern)
  if (changedFiles.some((file) => matcher.test(file))) {
    for (const doc of Array.isArray(rule.owned_by) ? rule.owned_by : []) {
      allowedDocs.add(doc)
    }
  }
}
for (const waiver of Array.isArray(parsed.waivers) ? parsed.waivers : []) {
  if (waiver && typeof waiver.doc === 'string') {
    allowedDocs.add(waiver.doc)
  }
}

for (const doc of Array.isArray(parsed.canonicalDocsUpdated) ? parsed.canonicalDocsUpdated : []) {
  if (!allowedDocs.has(doc)) {
    errors.push(`Canonical doc is not linked to the impacted subsystem: ${doc}`)
  }
}

if (errors.length > 0) {
  process.stdout.write(errors.join('\n'))
  process.exit(1)
}
EOF
)" || {
  printf '%s\n' "$validation_output"
  exit 1
}

echo "Decision link check passed."
