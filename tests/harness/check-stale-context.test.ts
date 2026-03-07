import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { execFileSync } from 'node:child_process'
import { afterEach, describe, expect, it } from 'vitest'

const scriptPath = path.resolve('scripts/harness/check_stale_context.sh')
const tempDirs: string[] = []

function writeFile(root: string, relativePath: string, content: string): void {
  const fullPath = path.join(root, relativePath)
  fs.mkdirSync(path.dirname(fullPath), { recursive: true })
  fs.writeFileSync(fullPath, content, 'utf8')
}

function initTempRepo(): string {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'stale-context-'))
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
`)
  writeFile(root, 'ARCHITECTURE.md', '# Architecture\n')
  writeFile(root, 'RELIABILITY.md', '# Reliability\n')
  writeFile(root, 'src/orchestrator/example.ts', 'export const example = 1\n')
  writeFile(root, 'docs/plans/2026-03-06-example-design.md', '# Example Design\n')
  writeFile(root, 'docs/generated/change-evidence/example.json', JSON.stringify({
    changedFiles: ['src/orchestrator/example.ts'],
    contextLoaded: ['ARCHITECTURE.md'],
    decisionArtifacts: ['docs/plans/missing-plan.md'],
    canonicalDocsUpdated: ['ARCHITECTURE.md'],
    waivers: [],
    verificationArtifacts: ['docs/generated/missing-verification.md'],
  }, null, 2))

  execFileSync('git', ['add', '.'], { cwd: root })
  execFileSync('git', ['commit', '-m', 'base'], { cwd: root })

  writeFile(root, 'src/orchestrator/example.ts', 'export const example = 2\n')
  execFileSync('git', ['add', '.'], { cwd: root })
  execFileSync('git', ['commit', '-m', 'orchestrator-change'], { cwd: root })

  return root
}

afterEach(() => {
  for (const root of tempDirs.splice(0)) {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

describe('check_stale_context.sh', () => {
  it('reports stale subsystem candidates, orphaned design docs, and missing evidence links', () => {
    const root = initTempRepo()
    const output = execFileSync('bash', [scriptPath], {
      cwd: root,
      env: { ...process.env, NODE_PATH: path.resolve('node_modules'), REPO_ROOT: root },
      encoding: 'utf8',
    })

    expect(output).toContain('Stale subsystem candidates')
    expect(output).toContain('src/orchestrator/**: 1 recent change(s) without owned doc updates')
    expect(output).toContain('Orphaned design docs')
    expect(output).toContain('2026-03-06-example-design.md -> missing 2026-03-06-example-implementation-plan.md')
    expect(output).toContain('Missing evidence links')
    expect(output).toContain('example.json -> missing docs/plans/missing-plan.md')
    expect(output).toContain('example.json -> missing docs/generated/missing-verification.md')
  })
})
