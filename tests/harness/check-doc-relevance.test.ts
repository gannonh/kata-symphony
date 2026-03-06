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

  execFileSync('git', ['init'], { cwd: root })
  execFileSync('git', ['config', 'user.name', 'Codex Test'], { cwd: root })
  execFileSync('git', ['config', 'user.email', 'codex@example.com'], { cwd: root })

  writeFile(root, 'ARCHITECTURE.md', '# Architecture\n\nLast reviewed: 2026-03-05\n\nInitial body.\n')
  writeFile(root, 'SECURITY.md', '# Security\n\nLast reviewed: 2026-03-05\n\nInitial body.\n')
  writeFile(root, 'docs/generated/change-evidence/task.json', JSON.stringify({
    contextLoaded: ['ARCHITECTURE.md'],
    canonicalDocsUpdated: ['ARCHITECTURE.md'],
    waivers: [],
  }, null, 2))

  execFileSync('git', ['add', '.'], { cwd: root })
  execFileSync('git', ['commit', '-m', 'base'], { cwd: root })
  return root
}

function runCheck(root: string): { ok: boolean; output: string } {
  try {
    const output = execFileSync('bash', [repoScript, 'HEAD~1'], {
      cwd: root,
      env: { ...process.env, REPO_ROOT: root, HARNESS_EVIDENCE_PATH: path.join(root, 'docs/generated/change-evidence/task.json') },
      encoding: 'utf8',
    })
    return { ok: true, output }
  } catch (error) {
    const output = error instanceof Error && 'stdout' in error ? String((error as { stdout?: string }).stdout ?? '') : ''
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
    writeFile(root, 'SECURITY.md', '# Security\n\nLast reviewed: 2026-03-06\n\nChanged without evidence linkage.\n')
    execFileSync('git', ['add', '.'], { cwd: root })
    execFileSync('git', ['commit', '-m', 'unrelated-doc'], { cwd: root })

    const result = runCheck(root)
    expect(result.ok).toBe(false)
    expect(result.output).toContain('Canonical doc changed outside evidence-declared context: SECURITY.md')
  })
})
