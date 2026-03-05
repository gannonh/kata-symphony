import { describe, expect, it } from 'vitest'
import { buildEffectiveConfig } from '../../src/config/build-effective-config.js'

describe('effective config builder', () => {
  it('maps workflow front matter to typed config with defaults', () => {
    const config = buildEffectiveConfig(
      { tracker: { kind: 'linear', project_slug: 'proj', api_key: '$LINEAR_API_KEY' } },
      { LINEAR_API_KEY: 'token' },
    )

    expect(config.tracker.endpoint).toBe('https://api.linear.app/graphql')
    expect(config.polling.interval_ms).toBe(30000)
  })

  it('throws typed error for missing required values', () => {
    expect(() => buildEffectiveConfig({ tracker: { kind: 'linear' } }, {})).toThrow(/tracker.api_key/)
  })
})
