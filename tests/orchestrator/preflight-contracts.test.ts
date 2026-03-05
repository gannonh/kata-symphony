import { describe, expect, it } from 'vitest'
import type {
  DispatchPreflightErrorCode,
  DispatchPreflightResult,
} from '../../src/orchestrator/preflight/index.js'
import { isDispatchPreflightFailure } from '../../src/orchestrator/preflight/index.js'
import * as orchestratorContracts from '../../src/orchestrator/contracts.js'

describe('dispatch preflight contracts', () => {
  it('exposes stable error codes and narrowing helper', () => {
    expect(typeof orchestratorContracts.isDispatchPreflightFailure).toBe('function')

    const codeMap: Record<DispatchPreflightErrorCode, true> = {
      workflow_invalid: true,
      tracker_kind_missing: true,
      tracker_kind_unsupported: true,
      tracker_api_key_missing: true,
      tracker_project_slug_missing: true,
      codex_command_missing: true,
    }

    const codeKeys = Object.keys(codeMap) as DispatchPreflightErrorCode[]
    expect(codeKeys).toHaveLength(6)
    expect(codeKeys).toEqual(
      expect.arrayContaining([
        'workflow_invalid',
        'tracker_kind_missing',
        'tracker_kind_unsupported',
        'tracker_api_key_missing',
        'tracker_project_slug_missing',
        'codex_command_missing',
      ]),
    )

    const makeResult = (shouldFail: boolean): DispatchPreflightResult =>
      shouldFail
        ? {
            ok: false,
            errors: [{ code: 'workflow_invalid', message: 'invalid workflow', source: 'workflow' }],
          }
        : { ok: true }

    const failureResult = makeResult(true)
    if (isDispatchPreflightFailure(failureResult)) {
      expect(failureResult.errors).toHaveLength(1)
      expect(failureResult.errors[0]?.code).toBe('workflow_invalid')
    } else {
      expect.unreachable('expected dispatch preflight failure')
    }

    const successResult = makeResult(false)
    if (isDispatchPreflightFailure(successResult)) {
      expect.unreachable('expected dispatch preflight success')
    } else {
      expect(successResult).toEqual({ ok: true })
    }
  })
})
