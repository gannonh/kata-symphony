import { describe, expect, it } from 'vitest'
import { createReloadableConfigProvider } from '../../src/config/reloadable-provider.js'
import type { WorkflowDefinition } from '../../src/domain/models.js'

const env: NodeJS.ProcessEnv = {
  HOME: '/Users/test',
  LINEAR_API_KEY: 'token',
}

function workflow(config: Record<string, unknown>): WorkflowDefinition {
  return {
    config,
    prompt_template: 'prompt',
  }
}

describe('reload boundaries', () => {
  it('applies updates only to future snapshots and preserves prior boundary reads', async () => {
    const provider = createReloadableConfigProvider(
      workflow({
        tracker: { kind: 'linear', project_slug: 'proj', api_key: '$LINEAR_API_KEY' },
        polling: { interval_ms: 30000 },
      }),
      env,
    )

    const dispatchBoundarySnapshot = provider.getSnapshot()

    const result = await provider.reload(
      workflow({
        tracker: { kind: 'linear', project_slug: 'proj', api_key: '$LINEAR_API_KEY' },
        polling: { interval_ms: 1000 },
      }),
    )

    expect(result.applied).toBe(true)
    expect(dispatchBoundarySnapshot.polling.interval_ms).toBe(30000)
    expect(provider.getSnapshot().polling.interval_ms).toBe(1000)
  })
})
