import fs from 'node:fs'
import { describe, expect, it } from 'vitest'

describe('lint baseline', () => {
  it('has eslint config and lint script uses eslint', () => {
    expect(fs.existsSync('eslint.config.js')).toBe(true)
    const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'))
    expect(pkg.scripts.lint).toContain('eslint')
  })
})

