import { spawnSync } from 'node:child_process'
import { describe, expect, it } from 'vitest'

const START_MESSAGE = 'Symphony bootstrap ok'

describe('startup command', () => {
  it('prints bootstrap message and exits 0', () => {
    const result = spawnSync('pnpm', ['start'], {
      encoding: 'utf8',
      env: process.env,
    })

    expect(result.status).toBe(0)
    expect(result.stdout).toContain(START_MESSAGE)
  })
})

