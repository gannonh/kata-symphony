#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$ROOT_DIR"

node - "$ROOT_DIR" <<'EOF'
const fs = require('node:fs')
const path = require('node:path')
const { execFileSync } = require('node:child_process')
const YAML = require('yaml')

const rootDir = process.argv[2]
const contextMapPath = path.join(rootDir, 'docs/harness/context-map.yaml')
const contextMap = YAML.parse(fs.readFileSync(contextMapPath, 'utf8'))
const rules = Array.isArray(contextMap?.rules) ? contextMap.rules : []

const globToRegExp = (pattern) => new RegExp(`^${pattern
  .replace(/[.+^${}()|[\]\\]/g, '\\$&')
  .replace(/\*\*/g, '::DOUBLE_STAR::')
  .replace(/\*/g, '[^/]*')
  .replace(/::DOUBLE_STAR::/g, '.*')}$`)

const logOutput = execFileSync(
  'git',
  ['log', '--max-count=20', '--pretty=format:__COMMIT__%n', '--name-only'],
  { cwd: rootDir, encoding: 'utf8' },
)

const commits = logOutput
  .split('__COMMIT__\n')
  .map((block) => block.split('\n').map((line) => line.trim()).filter(Boolean))
  .filter((files) => files.length > 0)

const staleSubsystems = []
for (const rule of rules) {
  const matcher = globToRegExp(rule.pattern)
  let changesWithoutDoc = 0

  for (const files of commits) {
    const touchedSubsystem = files.some((file) => matcher.test(file))
    if (!touchedSubsystem) {
      continue
    }

    const owns = Array.isArray(rule.owned_by) ? rule.owned_by : []
    const touchedOwnedDoc = files.some((file) => owns.includes(file))
    if (!touchedOwnedDoc) {
      changesWithoutDoc += 1
    }
  }

  if (changesWithoutDoc > 0) {
    staleSubsystems.push(`${rule.pattern}: ${changesWithoutDoc} recent change(s) without owned doc updates`)
  }
}

const plansDir = path.join(rootDir, 'docs/plans')
const orphanedPlans = []
if (fs.existsSync(plansDir)) {
  for (const file of fs.readdirSync(plansDir)) {
    if (!file.endsWith('-design.md')) {
      continue
    }

    const planFile = file.replace(/-design\.md$/, '-implementation-plan.md')
    if (!fs.existsSync(path.join(plansDir, planFile))) {
      orphanedPlans.push(`${file} -> missing ${planFile}`)
    }
  }
}

const evidenceDir = path.join(rootDir, 'docs/generated/change-evidence')
const missingEvidenceLinks = []
if (fs.existsSync(evidenceDir)) {
  for (const file of fs.readdirSync(evidenceDir)) {
    if (!file.endsWith('.json')) {
      continue
    }

    const parsed = JSON.parse(fs.readFileSync(path.join(evidenceDir, file), 'utf8'))
    const referenced = [
      ...(Array.isArray(parsed.changedFiles) ? parsed.changedFiles : []),
      ...(Array.isArray(parsed.contextLoaded) ? parsed.contextLoaded : []),
      ...(Array.isArray(parsed.decisionArtifacts) ? parsed.decisionArtifacts : []),
      ...(Array.isArray(parsed.canonicalDocsUpdated) ? parsed.canonicalDocsUpdated : []),
      ...(Array.isArray(parsed.verificationArtifacts) ? parsed.verificationArtifacts : []),
      ...((Array.isArray(parsed.waivers) ? parsed.waivers : []).flatMap((entry) =>
        entry && typeof entry.doc === 'string' ? [entry.doc] : [],
      )),
    ]

    for (const ref of referenced) {
      if (typeof ref !== 'string') {
        continue
      }
      if (!fs.existsSync(path.join(rootDir, ref))) {
        missingEvidenceLinks.push(`${file} -> missing ${ref}`)
      }
    }
  }
}

const printSection = (title, entries) => {
  console.log(title)
  if (entries.length === 0) {
    console.log('- none')
  } else {
    for (const entry of entries) {
      console.log(`- ${entry}`)
    }
  }
  console.log('')
}

printSection('Stale subsystem candidates', staleSubsystems)
printSection('Orphaned design docs', orphanedPlans)
printSection('Missing evidence links', missingEvidenceLinks)
EOF
