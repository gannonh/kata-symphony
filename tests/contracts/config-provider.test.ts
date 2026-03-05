import { describe, expect, it } from 'vitest'
import { createStaticConfigProvider } from '../../src/config/contracts.js'

describe('config provider', () => {
  it('returns snapshots isolated from source object mutation', () => {
    const source = { poll_interval_ms: 30000, max_concurrent_agents: 5 }
    const provider = createStaticConfigProvider(source)

    source.poll_interval_ms = 15000

    const snapshot = provider.getSnapshot()
    expect(snapshot.poll_interval_ms).toBe(30000)

    ;(snapshot as { poll_interval_ms: number }).poll_interval_ms = 500
    expect(provider.getSnapshot().poll_interval_ms).toBe(30000)
  })
})
