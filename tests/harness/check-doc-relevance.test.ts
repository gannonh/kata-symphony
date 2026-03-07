import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { afterEach, describe, expect, it } from 'vitest'

const repoScript = path.resolve('scripts/harness/check_doc_relevance.sh')
const tempDirs: string[] = []

function writeFile(root: string, relativePath: string, content: string): void {
  const fullPath = path.join(root, relativePath)
  fs.mkdirSync(path.dirname(fullPath), { recursive: true })
  fs.writeFileSync(fullPath, content, 'utf8')
}

function initTempRepo(): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'doc-relevance-'))
  tempDirs.push(root)

  execFileSync('git', ['init', '-b', 'main'], { cwd: root })
  execFileSync('git', ['config', 'user.name', 'Codex Test'], { cwd: root })
  execFileSync('git', ['config', 'user.email', 'codex@example.com'], { cwd: root })

  writeFile(root, 'ARCHITECTURE.md', '# Architecture\n\nLast reviewed: 2026-03-05\n\nInitial body.\n')
  writeFile(root, 'SECURITY.md', '# Security\n\nLast reviewed: 2026-03-05\n\nInitial body.\n')
  writeFile(root, 'docs/harness/context-map.yaml', [
    'rules:',
    '  - pattern: "ARCHITECTURE.md"',
    '    owned_by:',
    '      - "ARCHITECTURE.md"',
    '    impacted_areas:',
    '      - "architecture"',
    '  - pattern: "SECURITY.md"',
    '    owned_by:',
    '      - "SECURITY.md"',
    '    impacted_areas:',
    '      - "security"',
    '  - pattern: "scripts/harness/**"',
    '    owned_by:',
    '      - "docs/harness/BUILDING-WITH-HARNESS.md"',
    '    impacted_areas:',
    '      - "harness"',
  ].join('\n'))
  writeFile(root, 'docs/generated/change-evidence/task.json', JSON.stringify({
    changedFiles: ['ARCHITECTURE.md'],
    contextLoaded: ['ARCHITECTURE.md'],
    canonicalDocsUpdated: ['ARCHITECTURE.md'],
    waivers: [],
  }, null, 2))

  execFileSync('git', ['add', '.'], { cwd: root })
  execFileSync('git', ['commit', '-m', 'base'], { cwd: root })
  return root
}

function runCheck(
  root: string,
  options?: {
    args?: string[]
    env?: Record<string, string>
  },
): { ok: boolean; output: string } {
  try {
    const output = execFileSync('bash', [repoScript, ...(options?.args ?? ['HEAD~1'])], {
      cwd: root,
      env: {
        ...process.env,
        REPO_ROOT: root,
        HARNESS_EVIDENCE_PATH: path.join(root, 'docs/generated/change-evidence/task.json'),
        ...(options?.env ?? {}),
      },
      encoding: 'utf8',
    })
    return { ok: true, output }
  } catch (error) {
    const output = error instanceof Error && 'stdout' in error
      ? String((error as { stdout?: string; stderr?: string }).stdout ?? '') + String((error as { stdout?: string; stderr?: string }).stderr ?? '')
      : ''
    return { ok: false, output }
  }
}

afterEach(() => {
  for (const root of tempDirs.splice(0)) {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

describe('check_doc_relevance.sh', () => {
  it('fails when a canonical doc change only updates Last reviewed', () => {
    const root = initTempRepo()
    writeFile(root, 'ARCHITECTURE.md', '# Architecture\n\nLast reviewed: 2026-03-06\n\nInitial body.\n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'metadata-only'], { cwd: root })

    const result = runCheck(root)
    expect(result.ok).toBe(false)
    expect(result.output).toContain('Canonical doc change is metadata-only: ARCHITECTURE.md')
  })

  it('fails when a canonical doc change only alters whitespace', () => {
    const root = initTempRepo()
    writeFile(root, 'ARCHITECTURE.md', '# Architecture\n\nLast reviewed: 2026-03-05\n\nInitial body.   \n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'whitespace-only'], { cwd: root })

    const result = runCheck(root)
    expect(result.ok).toBe(false)
    expect(result.output).toContain('Canonical doc change is metadata-only: ARCHITECTURE.md')
  })

  it('passes when a canonical doc has a substantive body update', () => {
    const root = initTempRepo()
    writeFile(root, 'ARCHITECTURE.md', '# Architecture\n\nLast reviewed: 2026-03-06\n\nInitial body.\n\nAdded harness notes.\n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'substantive-update'], { cwd: root })

    const result = runCheck(root)
    expect(result.ok).toBe(true)
    expect(result.output).toContain('Doc relevance check passed.')
  })

  it('fails when an unrelated canonical doc is changed outside evidence-declared context', () => {
    const root = initTempRepo()
    writeFile(root, 'docs/generated/change-evidence/task.json', JSON.stringify({
      changedFiles: ['SECURITY.md', 'docs/generated/change-evidence/task.json'],
      contextLoaded: ['ARCHITECTURE.md'],
      canonicalDocsUpdated: ['ARCHITECTURE.md'],
      waivers: [],
    }, null, 2))
    writeFile(root, 'SECURITY.md', '# Security\n\nLast reviewed: 2026-03-06\n\nChanged without evidence linkage.\n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'unrelated-doc'], { cwd: root })

    const result = runCheck(root)
    expect(result.ok).toBe(false)
    expect(result.output).toContain('Canonical doc changed without declared update or waiver linkage: SECURITY.md')
  })

  it('fails when contextLoaded names a canonical doc but the doc is not declared updated or waived', () => {
    const root = initTempRepo()
    writeFile(root, 'docs/generated/change-evidence/task.json', JSON.stringify({
      changedFiles: ['SECURITY.md', 'docs/generated/change-evidence/task.json'],
      contextLoaded: ['SECURITY.md'],
      canonicalDocsUpdated: [],
      waivers: [],
    }, null, 2))
    writeFile(root, 'SECURITY.md', '# Security\n\nLast reviewed: 2026-03-06\n\nChanged after loading only.\n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'context-only'], { cwd: root })

    const result = runCheck(root)
    expect(result.ok).toBe(false)
    expect(result.output).toContain('Canonical doc changed without declared update or waiver linkage: SECURITY.md')
  })

  it('uses the evidence artifact that matches the current diff instead of the latest file name', () => {
    const root = initTempRepo()
    writeFile(root, 'docs/generated/change-evidence/zeta.json', JSON.stringify({
      changedFiles: ['ARCHITECTURE.md'],
      contextLoaded: ['SECURITY.md'],
      canonicalDocsUpdated: ['SECURITY.md'],
      waivers: [],
    }, null, 2))
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'seed-unrelated-evidence'], { cwd: root })

    writeFile(root, 'docs/generated/change-evidence/task.json', JSON.stringify({
      changedFiles: ['SECURITY.md', 'docs/generated/change-evidence/task.json'],
      contextLoaded: ['SECURITY.md'],
      canonicalDocsUpdated: [],
      waivers: [],
    }, null, 2))
    writeFile(root, 'docs/generated/change-evidence/zeta.json', JSON.stringify({
      changedFiles: ['ARCHITECTURE.md'],
      contextLoaded: ['SECURITY.md'],
      canonicalDocsUpdated: ['SECURITY.md'],
      waivers: [],
    }, null, 2))
    writeFile(root, 'SECURITY.md', '# Security\n\nLast reviewed: 2026-03-06\n\nChanged with wrong artifact also present.\n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'multiple-evidence'], { cwd: root })

    const result = runCheck(root, {
      env: { HARNESS_EVIDENCE_PATH: '' },
    })
    expect(result.ok).toBe(false)
    expect(result.output).toContain('Canonical doc changed without declared update or waiver linkage: SECURITY.md')
  })

  it('validates the whole branch relative to main instead of only the tip commit', () => {
    const root = initTempRepo()
    execFileSync('git', ['switch', '-c', 'feature/harness-fix'], { cwd: root })

    writeFile(root, 'docs/generated/change-evidence/task.json', JSON.stringify({
      changedFiles: [
        'SECURITY.md',
        'docs/generated/change-evidence/task.json',
        'scripts/harness/example.sh',
      ],
      contextLoaded: ['docs/harness/BUILDING-WITH-HARNESS.md'],
      canonicalDocsUpdated: [],
      waivers: [],
    }, null, 2))
    writeFile(root, 'SECURITY.md', '# Security\n\nLast reviewed: 2026-03-06\n\nEarlier branch commit changed this doc.\n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'earlier-branch-doc-change'], { cwd: root })

    writeFile(root, 'scripts/harness/example.sh', '#!/usr/bin/env bash\necho later\n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'later-branch-code-change'], { cwd: root })

    const result = runCheck(root, {
      args: [],
      env: {
        HARNESS_EVIDENCE_PATH: path.join(root, 'docs/generated/change-evidence/task.json'),
      },
    })
    expect(result.ok).toBe(false)
    expect(result.output).toContain('Canonical doc changed without declared update or waiver linkage: SECURITY.md')
  })
})
