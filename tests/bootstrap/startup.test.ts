import { spawnSync } from 'node:child_process'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

const START_MESSAGE = 'Symphony bootstrap ok'
const TEST_DIR = dirname(fileURLToPath(import.meta.url))
const MAIN_ENTRY_PATH = fileURLToPath(
  new URL('../../src/main.ts', import.meta.url),
)

describe('startup command', () => {
  it('prints bootstrap message and exits 0', () => {
    const result = spawnSync('pnpm', ['start'], {
      encoding: 'utf8',
      env: { ...process.env, LINEAR_API_KEY: 'test-bootstrap-key' },
    })

    expect(result.status).toBe(0)
    expect(result.stdout).toContain(START_MESSAGE)
  })

  it('exits 1 and reports startup failure when startup preflight blocks bootstrap', () => {
    const isolatedCwd = mkdtempSync(join(tmpdir(), 'symphony-startup-'))
    const result = spawnSync(
      'pnpm',
      [
        'exec',
        'tsx',
        '-e',
        `void (async () => { process.chdir(${JSON.stringify(isolatedCwd)}); await import(${JSON.stringify(MAIN_ENTRY_PATH)}); })();`,
      ],
      {
        encoding: 'utf8',
        cwd: join(TEST_DIR, '../..'),
        env: { ...process.env, LINEAR_API_KEY: 'test-bootstrap-key' },
      },
    )

    try {
      expect(result.status).toBe(1)
      expect(result.stdout).not.toContain(START_MESSAGE)
      expect(result.stderr).toContain('Dispatch preflight validation failed')
      expect(result.stderr).toContain('workflow_invalid')
      expect(result.stderr).toContain('dispatch_preflight_failed')
    } finally {
      rmSync(isolatedCwd, { recursive: true, force: true })
    }
  })
})
