import { describe, expect, it, vi } from 'vitest'
import { createReloadableConfigProvider } from '../../src/config/reloadable-provider.js'
import { ConfigValidationError } from '../../src/config/errors.js'
import type { WorkflowDefinition } from '../../src/domain/models.js'

const env: NodeJS.ProcessEnv = {
  HOME: '/Users/test',
  LINEAR_API_KEY: 'token',
}

const validWorkflow: WorkflowDefinition = {
  config: {
    tracker: {
      kind: 'linear',
      project_slug: 'proj',
      api_key: '$LINEAR_API_KEY',
    },
  },
  prompt_template: 'prompt',
}

const invalidWorkflow: WorkflowDefinition = {
  config: {
    tracker: {
      kind: 'linear',
      project_slug: 'proj',
      api_key: '$MISSING',
    },
  },
  prompt_template: 'prompt',
}

describe('reloadable config provider', () => {
  it('keeps last known good snapshot when reload input is invalid', async () => {
    const provider = createReloadableConfigProvider(validWorkflow, env)
    const before = provider.getSnapshot()

    const result = await provider.reload(invalidWorkflow)
    expect(result.applied).toBe(false)
    if (!result.applied) {
      expect(result.error).toBeDefined()
      expect(result.error).toBeInstanceOf(ConfigValidationError)
    }
    expect(provider.getSnapshot()).toEqual(before)
  })

  it('rethrows unexpected runtime errors from reload', async () => {
    const provider = createReloadableConfigProvider(validWorkflow, env)

    const buildMod = await import('../../src/config/build-effective-config.js')
    const runtimeError = new TypeError('unexpected runtime fault')
    vi.spyOn(buildMod, 'buildEffectiveConfig').mockImplementation(() => {
      throw runtimeError
    })

    await expect(provider.reload(validWorkflow)).rejects.toThrow(runtimeError)
    vi.restoreAllMocks()
  })
})
