import { spawnSync } from 'node:child_process'
import { describe, expect, it } from 'vitest'

const START_MESSAGE = 'Symphony bootstrap ok'

describe('startup command', () => {
  it('prints bootstrap message and exits 0', () => {
    const result = spawnSync('pnpm', ['start'], {
      encoding: 'utf8',
      env: { ...process.env, LINEAR_API_KEY: 'test-bootstrap-key' },
    })

    expect(result.status).toBe(0)
    expect(result.stdout).toContain(START_MESSAGE)
  })

  it('exits 1 and reports startup failure when bootstrap config is invalid', () => {
    const result = spawnSync('pnpm', ['start'], {
      encoding: 'utf8',
      env: { ...process.env, LINEAR_API_KEY: '' },
    })

    expect(result.status).toBe(1)
    expect(result.stdout).not.toContain(START_MESSAGE)
    expect(result.stderr).toContain('Symphony startup failed')
    expect(result.stderr).toContain('tracker.api_key is required after $VAR resolution')
  })
})
