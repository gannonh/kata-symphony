import { describe, expect, it } from 'vitest'
import { validateDispatchPreflight } from '../../src/orchestrator/preflight/index.js'

describe('validateDispatchPreflight', () => {
  it('maps workflow loader failure to workflow_invalid', async () => {
    const result = await validateDispatchPreflight({
      loadWorkflow: async () => {
        throw new Error('bad yaml')
      },
      getSnapshot: () =>
        ({
          tracker: { kind: 'linear', api_key: 'k', project_slug: 'proj' },
          codex: { command: 'codex app-server' },
        }) as never,
    })

    expect(result).toMatchObject({
      ok: false,
      errors: [{ code: 'workflow_invalid', source: 'workflow' }],
    })
  })

  it('returns all config failures in deterministic order', async () => {
    const result = await validateDispatchPreflight({
      loadWorkflow: async () => ({ config: {}, prompt_template: 'ok' }),
      getSnapshot: () =>
        ({
          tracker: { kind: '' as never, api_key: ' ', project_slug: ' ' },
          codex: { command: '   ' },
        }) as never,
    })

    expect(result).toMatchObject({
      ok: false,
      errors: [
        { code: 'tracker_kind_missing' },
        { code: 'tracker_api_key_missing' },
        { code: 'tracker_project_slug_missing' },
        { code: 'codex_command_missing' },
      ],
    })
  })
})
