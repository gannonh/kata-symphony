import fs from 'node:fs'
import { describe, expect, it } from 'vitest'

const requiredDirs = [
  'src/config',
  'src/tracker',
  'src/orchestrator',
  'src/execution',
  'src/observability',
]

describe('scaffold layout', () => {
  it('contains required layer directories', () => {
    for (const dir of requiredDirs) {
      expect(fs.existsSync(dir), `${dir} missing`).toBe(true)
    }
  })
})

