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

    expect(result).toEqual({
      ok: false,
      errors: [
        {
          code: 'workflow_invalid',
          message: 'Workflow file cannot be loaded or parsed',
          source: 'workflow',
          field: 'workflow',
        },
      ],
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

    expect(result).toMatchObject({ ok: false })
    expect(result.ok).toBe(false)
    expect(result.errors.map((error) => error.code)).toEqual([
      'tracker_kind_missing',
      'tracker_api_key_missing',
      'tracker_project_slug_missing',
      'codex_command_missing',
    ])
  })

  it('returns config validation errors instead of throwing when getSnapshot fails', async () => {
    const result = await validateDispatchPreflight({
      loadWorkflow: async () => ({ config: {}, prompt_template: 'ok' }),
      getSnapshot: () => {
        throw new Error('snapshot unavailable')
      },
    })

    expect(result).toMatchObject({ ok: false })
    expect(result.ok).toBe(false)
    expect(result.errors.map((error) => error.code)).toEqual([
      'tracker_kind_missing',
      'tracker_api_key_missing',
      'tracker_project_slug_missing',
      'codex_command_missing',
    ])
  })

  it('returns tracker_kind_unsupported for non-linear trackers', async () => {
    const result = await validateDispatchPreflight({
      loadWorkflow: async () => ({ config: {}, prompt_template: 'ok' }),
      getSnapshot: () =>
        ({
          tracker: { kind: 'jira', api_key: 'k', project_slug: '' },
          codex: { command: 'codex app-server' },
        }) as never,
    })

    expect(result).toMatchObject({ ok: false })
    expect(result.ok).toBe(false)
    expect(result.errors.map((error) => error.code)).toEqual([
      'tracker_kind_unsupported',
    ])
  })
})
