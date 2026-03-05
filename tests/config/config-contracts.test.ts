import { describe, expect, it } from 'vitest'
import type { EffectiveConfig } from '../../src/config/types.js'

describe('config contracts', () => {
  it('exposes typed config sections', async () => {
    const mod = await import('../../src/config/contracts.js')
    expect(mod).toHaveProperty('createStaticConfigProvider')

    const snapshot = mod.createStaticConfigProvider({} as EffectiveConfig).getSnapshot()
    expect(snapshot).toHaveProperty('tracker')
    expect(snapshot).toHaveProperty('polling')
    expect(snapshot).toHaveProperty('workspace')
    expect(snapshot).toHaveProperty('hooks')
    expect(snapshot).toHaveProperty('agent')
    expect(snapshot).toHaveProperty('codex')
  })
})
