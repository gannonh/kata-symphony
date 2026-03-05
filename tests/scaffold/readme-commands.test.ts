import fs from 'node:fs'
import { describe, expect, it } from 'vitest'

const requiredSnippets = [
  'pnpm install',
  'pnpm run lint',
  'pnpm run typecheck',
  'pnpm test',
  'pnpm start',
]

describe('readme setup commands', () => {
  it('documents required pnpm commands', () => {
    const readme = fs.readFileSync('README.md', 'utf8')
    for (const snippet of requiredSnippets) {
      expect(readme).toContain(snippet)
    }
  })
})

