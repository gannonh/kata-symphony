import { describe, expect, it } from 'vitest'
import type { ConfigSnapshot } from '../../src/config/contracts.js'
import type { DispatchPreflightResult } from '../../src/orchestrator/preflight/index.js'
import {
  isDispatchPreflightFailure,
  validateDispatchPreflight,
} from '../../src/orchestrator/preflight/index.js'

function asSnapshot(value: unknown): ConfigSnapshot {
  return value as ConfigSnapshot
}

function expectPreflightFailure(
  result: DispatchPreflightResult,
): Extract<DispatchPreflightResult, { ok: false }> {
  expect(result.ok).toBe(false)
  if (isDispatchPreflightFailure(result)) {
    return result
  }

  expect.unreachable('expected preflight failure result')
}

describe('validateDispatchPreflight', () => {
  it('maps workflow loader failure to workflow_invalid', async () => {
    const result = await validateDispatchPreflight({
      loadWorkflow: async () => {
        throw new Error('bad yaml')
      },
      getSnapshot: () =>
        asSnapshot({
          tracker: { kind: 'linear', api_key: 'k', project_slug: 'proj' },
          codex: { command: 'codex app-server' },
        }),
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
        asSnapshot({
          tracker: { kind: '', api_key: ' ', project_slug: ' ' },
          codex: { command: '   ' },
        }),
    })

    const failure = expectPreflightFailure(result)
    expect(failure.errors.map((error) => error.code)).toEqual([
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

    const failure = expectPreflightFailure(result)
    expect(failure.errors.map((error) => error.code)).toEqual([
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
        asSnapshot({
          tracker: { kind: 'jira', api_key: 'k', project_slug: '' },
          codex: { command: 'codex app-server' },
        }),
    })

    const failure = expectPreflightFailure(result)
    expect(failure.errors.map((error) => error.code)).toEqual([
      'tracker_kind_unsupported',
    ])
  })
})
