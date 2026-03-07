import fs from 'node:fs'
import { describe, expect, it } from 'vitest'

describe('package scripts', () => {
  it('defines required scripts', () => {
    const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'))
    expect(pkg.scripts).toMatchObject({
      'harness:generate-evidence': expect.any(String),
      lint: expect.any(String),
      typecheck: expect.any(String),
      test: expect.any(String),
      start: expect.any(String),
      dev: expect.any(String),
    })
  })
})
