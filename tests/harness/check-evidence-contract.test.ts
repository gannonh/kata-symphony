import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { afterEach, describe, expect, it } from 'vitest'

const evidenceScript = path.resolve('scripts/harness/check_evidence_contract.sh')
const linksScript = path.resolve('scripts/harness/check_decision_links.sh')
const tempDirs: string[] = []

function writeFile(root: string, relativePath: string, content: string): void {
  const fullPath = path.join(root, relativePath)
  fs.mkdirSync(path.dirname(fullPath), { recursive: true })
  fs.writeFileSync(fullPath, content, 'utf8')
}

function initTempRepo(): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'evidence-contract-'))
  tempDirs.push(root)

  execFileSync('git', ['init'], { cwd: root })
  execFileSync('git', ['config', 'user.name', 'Codex Test'], { cwd: root })
  execFileSync('git', ['config', 'user.email', 'codex@example.com'], { cwd: root })

  writeFile(root, 'docs/harness/context-map.yaml', `rules:
  - pattern: "src/orchestrator/**"
    owned_by:
      - "ARCHITECTURE.md"
      - "RELIABILITY.md"
    impacted_areas:
      - "architecture"
      - "reliability"
  - pattern: "scripts/harness/**"
    owned_by:
      - "docs/harness/BUILDING-WITH-HARNESS.md"
      - "docs/harness/change-evidence-schema.md"
    impacted_areas:
      - "harness"
      - "documentation"
`)
  writeFile(root, 'ARCHITECTURE.md', '# Architecture\n')
  writeFile(root, 'RELIABILITY.md', '# Reliability\n')
  writeFile(root, 'docs/harness/BUILDING-WITH-HARNESS.md', '# Harness\n')
  writeFile(root, 'docs/harness/change-evidence-schema.md', '# Schema\n')
  writeFile(root, 'docs/plans/task-design.md', '# Task Design\n')
  writeFile(root, 'docs/generated/task-verification.md', '# Verification\n')
  writeFile(root, 'src/orchestrator/example.ts', 'export const example = 1\n')
  writeFile(root, 'scripts/harness/example.sh', '#!/usr/bin/env bash\necho ok\n')

  execFileSync('git', ['add', '.'], { cwd: root })
  execFileSync('git', ['commit', '-m', 'base'], { cwd: root })
  return root
}

function writeEvidence(root: string, content: Record<string, unknown>): string {
  const jsonPath = path.join(root, 'docs/generated/change-evidence/task.json')
  writeFile(root, 'docs/generated/change-evidence/task.json', `${JSON.stringify(content, null, 2)}\n`)
  writeFile(
    root,
    'docs/generated/change-evidence/task.md',
    `# Change Evidence: task

## Summary

task

## Changed Files

- \`${(content.changedFiles as string[] | undefined)?.[0] ?? 'missing'}\`

## Context Loaded

${((content.contextLoaded as string[] | undefined) ?? []).map((item) => `- \`${item}\``).join('\n')}

## Decision Artifacts

${((content.decisionArtifacts as string[] | undefined) ?? []).map((item) => `- \`${item}\``).join('\n')}

## Canonical Docs Updated

${((content.canonicalDocsUpdated as string[] | undefined) ?? []).map((item) => `- \`${item}\``).join('\n')}

## Waivers

${((content.waivers as Array<{ doc: string; reason: string }> | undefined) ?? []).map((item) => `- \`${item.doc}\`: ${item.reason}`).join('\n')}

## Verification

${((content.verification as Array<{ command: string; result: string }> | undefined) ?? []).map((item) => `- \`${item.command}\` -> ${item.result}`).join('\n')}

## Verification Artifacts

${((content.verificationArtifacts as string[] | undefined) ?? []).map((item) => `- \`${item}\``).join('\n')}
`,
  )

  return jsonPath
}

function runScript(scriptPath: string, root: string, evidencePath?: string): { ok: boolean; output: string } {
  try {
    const output = execFileSync('bash', [scriptPath, 'HEAD~1'], {
      cwd: root,
      env: {
        ...process.env,
        NODE_PATH: path.resolve('node_modules'),
        REPO_ROOT: root,
        ...(evidencePath ? { HARNESS_EVIDENCE_PATH: evidencePath } : {}),
      },
      encoding: 'utf8',
    })
    return { ok: true, output }
  } catch (error) {
    const stdout = error instanceof Error && 'stdout' in error ? String((error as { stdout?: string }).stdout ?? '') : ''
    return { ok: false, output: stdout }
  }
}

afterEach(() => {
  for (const root of tempDirs.splice(0)) {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

describe('evidence contract harness scripts', () => {
  it('fails when qualifying code changes have no evidence manifest', () => {
    const root = initTempRepo()
    writeFile(root, 'scripts/harness/example.sh', '#!/usr/bin/env bash\necho changed\n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'change'], { cwd: root })

    const result = runScript(evidenceScript, root)
    expect(result.ok).toBe(false)
    expect(result.output).toContain('Qualifying changes require a change-evidence JSON artifact.')
  })

  it('fails when the evidence manifest is missing required fields', () => {
    const root = initTempRepo()
    writeFile(root, 'scripts/harness/example.sh', '#!/usr/bin/env bash\necho changed\n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'change'], { cwd: root })

    const evidencePath = writeEvidence(root, {
      topic: 'task',
      summary: 'task',
      changedFiles: ['scripts/harness/example.sh'],
      contextLoaded: ['docs/harness/BUILDING-WITH-HARNESS.md'],
      decisionArtifacts: [],
      canonicalDocsUpdated: [],
      waivers: [],
      verification: [],
      verificationArtifacts: [],
      impactedAreas: [],
    })

    const result = runScript(evidenceScript, root, evidencePath)
    expect(result.ok).toBe(false)
    expect(result.output).toContain('Architecture-sensitive changes require at least one decision artifact.')
  })

  it('fails when verification command evidence is missing', () => {
    const root = initTempRepo()
    writeFile(root, 'scripts/harness/example.sh', '#!/usr/bin/env bash\necho changed\n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'change'], { cwd: root })

    const evidencePath = writeEvidence(root, {
      topic: 'task',
      summary: 'task',
      changedFiles: ['scripts/harness/example.sh'],
      contextLoaded: ['docs/harness/BUILDING-WITH-HARNESS.md'],
      decisionArtifacts: ['docs/plans/task-design.md'],
      canonicalDocsUpdated: ['docs/harness/BUILDING-WITH-HARNESS.md'],
      waivers: [],
      verification: [{ command: 'fill-in-command', result: 'pending' }],
      verificationArtifacts: ['docs/generated/task-verification.md'],
      impactedAreas: ['harness'],
    })

    const result = runScript(evidenceScript, root, evidencePath)
    expect(result.ok).toBe(false)
    expect(result.output).toContain('Verification entries must include concrete command evidence.')
  })

  it('passes with an explicit no-doc waiver that includes rationale', () => {
    const root = initTempRepo()
    writeFile(root, 'scripts/harness/example.sh', '#!/usr/bin/env bash\necho changed\n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'change'], { cwd: root })

    const evidencePath = writeEvidence(root, {
      topic: 'task',
      summary: 'task',
      changedFiles: ['scripts/harness/example.sh'],
      contextLoaded: ['docs/harness/BUILDING-WITH-HARNESS.md'],
      decisionArtifacts: ['docs/plans/task-design.md'],
      canonicalDocsUpdated: [],
      waivers: [{ doc: 'docs/harness/BUILDING-WITH-HARNESS.md', reason: 'No durable doc delta for this fixture.' }],
      verification: [{ command: 'bash scripts/harness/example.sh', result: 'pass' }],
      verificationArtifacts: ['docs/generated/task-verification.md'],
      impactedAreas: ['harness'],
    })

    const result = runScript(evidenceScript, root, evidencePath)
    expect(result.ok).toBe(true)
    expect(result.output).toContain('Evidence contract check passed.')
  })

  it('fails decision-link validation when linked artifacts are missing', () => {
    const root = initTempRepo()
    writeFile(root, 'src/orchestrator/example.ts', 'export const example = 2\n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'change'], { cwd: root })

    const evidencePath = writeEvidence(root, {
      topic: 'task',
      summary: 'task',
      changedFiles: ['src/orchestrator/example.ts'],
      contextLoaded: ['ARCHITECTURE.md', 'RELIABILITY.md'],
      decisionArtifacts: ['docs/plans/missing.md'],
      canonicalDocsUpdated: ['ARCHITECTURE.md'],
      waivers: [],
      verification: [{ command: 'pnpm vitest', result: 'pass' }],
      verificationArtifacts: ['docs/generated/task-verification.md'],
      impactedAreas: ['architecture', 'reliability'],
    })

    const result = runScript(linksScript, root, evidencePath)
    expect(result.ok).toBe(false)
    expect(result.output).toContain('Linked artifact does not exist: docs/plans/missing.md')
  })
})
