import fs from 'node:fs'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import YAML from 'yaml'

interface ContextRule {
  pattern: string
  owned_by: string[]
  impacted_areas: string[]
  notes?: string
}

interface ContextMap {
  rules: ContextRule[]
}

interface VerificationEntry {
  command: string
  result: 'pending' | 'pass' | 'fail'
}

interface WaiverEntry {
  doc: string
  reason: string
}

export interface ChangeEvidence {
  topic: string
  summary: string
  changedFiles: string[]
  contextLoaded: string[]
  decisionArtifacts: string[]
  canonicalDocsUpdated: string[]
  waivers: WaiverEntry[]
  verification: VerificationEntry[]
  verificationArtifacts: string[]
  impactedAreas: string[]
}

function globToRegExp(pattern: string): RegExp {
  const escaped = pattern
    .replace(/[.+^${}()|[\]\\]/g, '\\$&')
    .replace(/\*\*/g, '::DOUBLE_STAR::')
    .replace(/\*/g, '[^/]*')
    .replace(/::DOUBLE_STAR::/g, '.*')

  return new RegExp(`^${escaped}$`)
}

function uniqueSorted(values: string[]): string[] {
  return [...new Set(values)].sort((a, b) => a.localeCompare(b))
}

export function loadContextMap(repoRoot: string): ContextMap {
  const raw = fs.readFileSync(path.join(repoRoot, 'docs/harness/context-map.yaml'), 'utf8')
  const parsed = YAML.parse(raw) as Partial<ContextMap> | null
  return {
    rules: Array.isArray(parsed?.rules) ? parsed.rules : [],
  }
}

export function inferEvidence(topic: string, changedFiles: string[], contextMap: ContextMap): ChangeEvidence {
  const matchedRules = contextMap.rules.filter((rule) => {
    const matcher = globToRegExp(rule.pattern)
    return changedFiles.some((file) => matcher.test(file))
  })

  return {
    topic,
    summary: `Change evidence stub for ${topic}`,
    changedFiles: uniqueSorted(changedFiles),
    contextLoaded: uniqueSorted(matchedRules.flatMap((rule) => rule.owned_by)),
    decisionArtifacts: [],
    canonicalDocsUpdated: [],
    waivers: [],
    verification: [
      {
        command: 'fill-in-command',
        result: 'pending',
      },
    ],
    verificationArtifacts: [],
    impactedAreas: uniqueSorted(matchedRules.flatMap((rule) => rule.impacted_areas)),
  }
}

export function renderMarkdownEvidence(evidence: ChangeEvidence): string {
  const bulletList = (items: string[], emptyText: string): string =>
    items.length > 0 ? items.map((item) => `- \`${item}\``).join('\n') : `- ${emptyText}`

  const waiverList =
    evidence.waivers.length > 0
      ? evidence.waivers.map((entry) => `- \`${entry.doc}\`: ${entry.reason}`).join('\n')
      : '- None yet. Add explicit doc waivers if required.'

  const verificationList =
    evidence.verification.length > 0
      ? evidence.verification.map((entry) => `- \`${entry.command}\` -> ${entry.result}`).join('\n')
      : '- No verification recorded yet.'

  return `# Change Evidence: ${evidence.topic}

## Summary

${evidence.summary}

## Changed Files

${bulletList(evidence.changedFiles, 'No changed files detected.')}

## Context Loaded

${bulletList(evidence.contextLoaded, 'No required context inferred.')}

## Decision Artifacts

${bulletList(evidence.decisionArtifacts, 'None yet. Add linked design docs or implementation plans here.')}

## Canonical Docs Updated

${bulletList(evidence.canonicalDocsUpdated, 'None yet. Add updated durable docs here.')}

## Waivers

${waiverList}

## Verification

${verificationList}

## Verification Artifacts

${bulletList(evidence.verificationArtifacts, 'None yet. Add linked verification docs here.')}

## Impacted Areas

${bulletList(evidence.impactedAreas, 'No impacted areas inferred.')}
`
}

function detectChangedFiles(repoRoot: string): string[] {
  const output = execFileSync(
    'git',
    ['diff', '--name-only', 'HEAD'],
    { cwd: repoRoot, encoding: 'utf8' },
  )

  return output
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
}

export function writeEvidenceFiles(repoRoot: string, evidence: ChangeEvidence): { jsonPath: string; markdownPath: string } {
  const outputDir = path.join(repoRoot, 'docs/generated/change-evidence')
  fs.mkdirSync(outputDir, { recursive: true })

  const stamp = new Date().toISOString().slice(0, 10)
  const baseName = `${stamp}-${evidence.topic}`
  const jsonPath = path.join(outputDir, `${baseName}.json`)
  const markdownPath = path.join(outputDir, `${baseName}.md`)

  fs.writeFileSync(jsonPath, `${JSON.stringify(evidence, null, 2)}\n`, 'utf8')
  fs.writeFileSync(markdownPath, renderMarkdownEvidence(evidence), 'utf8')

  return { jsonPath, markdownPath }
}

function main(): void {
  const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
  const [topic = 'change-evidence', ...fileArgs] = process.argv.slice(2)
  const changedFiles = fileArgs.length > 0 ? fileArgs : detectChangedFiles(repoRoot)
  const evidence = inferEvidence(topic, changedFiles, loadContextMap(repoRoot))
  const written = writeEvidenceFiles(repoRoot, evidence)

  process.stdout.write(`${written.jsonPath}\n${written.markdownPath}\n`)
}

const executedDirectly = process.argv[1] !== undefined
  && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)

if (executedDirectly) {
  main()
}
