import { describe, expect, it } from 'vitest'
import type { DispatchPreflightErrorCode } from '../../src/orchestrator/preflight/index.js'
import { isDispatchPreflightFailure } from '../../src/orchestrator/preflight/index.js'

describe('dispatch preflight contracts', () => {
  it('exposes stable error codes and narrowing helper', () => {
    const codes: DispatchPreflightErrorCode[] = [
      'workflow_invalid',
      'tracker_kind_missing',
      'tracker_kind_unsupported',
      'tracker_api_key_missing',
      'tracker_project_slug_missing',
      'codex_command_missing',
    ]

    expect(codes).toHaveLength(6)
    expect(isDispatchPreflightFailure({ ok: false, errors: [] })).toBe(true)
  })
})
