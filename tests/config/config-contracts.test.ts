import { describe, expect, it } from 'vitest'
import type { EffectiveConfig } from '../../src/config/types.js'
import { DEFAULTS } from '../../src/config/defaults.js'

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

  it('returns deep-cloned snapshots for nested sections', async () => {
    const mod = await import('../../src/config/contracts.js')
    const provider = mod.createStaticConfigProvider({
      tracker: { kind: 'linear' },
    })

    const snapshot = provider.getSnapshot()
    ;(snapshot.tracker as { kind: string }).kind = 'changed'

    expect((provider.getSnapshot().tracker as { kind: string }).kind).toBe('linear')
  })

  it('preserves nested defaults when applying partial section overrides', async () => {
    const mod = await import('../../src/config/contracts.js')
    const snapshot = mod.createStaticConfigProvider({
      tracker: { kind: 'linear' },
      codex: { command: 'codex run' },
    }).getSnapshot()

    expect(snapshot.tracker.kind).toBe('linear')
    expect(snapshot.tracker.endpoint).toBe(DEFAULTS.tracker.endpoint)
    expect(snapshot.tracker.active_states).toEqual(DEFAULTS.tracker.active_states)
    expect(snapshot.codex.command).toBe('codex run')
    expect(snapshot.codex.turn_timeout_ms).toBe(DEFAULTS.codex.turn_timeout_ms)
  })

  it('yields canonical defaults from createStaticConfigProvider({})', async () => {
    const mod = await import('../../src/config/contracts.js')
    const snapshot = mod.createStaticConfigProvider({}).getSnapshot()

    expect(snapshot.tracker.kind).toBe(DEFAULTS.tracker.kind)
    expect(snapshot.tracker.endpoint).toBe(DEFAULTS.tracker.endpoint)
    expect(snapshot.polling.interval_ms).toBe(DEFAULTS.polling.interval_ms)
    expect(snapshot.agent.max_concurrent_agents).toBe(DEFAULTS.agent.max_concurrent_agents)
    expect(snapshot.agent.max_turns).toBe(DEFAULTS.agent.max_turns)
    expect(snapshot.codex.command).toBe(DEFAULTS.codex.command)
    expect(snapshot.codex.turn_timeout_ms).toBe(DEFAULTS.codex.turn_timeout_ms)
    expect(snapshot.hooks.timeout_ms).toBe(DEFAULTS.hooks.timeout_ms)
  })
})
