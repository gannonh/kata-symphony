import { describe, expect, it, vi } from 'vitest'
import type { DispatchPreflightError } from '../../src/orchestrator/preflight/contracts.js'
import { runTickPreflightGate } from '../../src/orchestrator/preflight/index.js'

describe('runTickPreflightGate', () => {
  it('runs reconcile before validate and blocks dispatch on preflight failure', async () => {
    const errors: DispatchPreflightError[] = [
      {
        code: 'workflow_invalid',
        source: 'workflow',
        field: 'workflow',
        message: 'Workflow file cannot be loaded or parsed',
      },
    ]

    const reconcile = vi.fn(async () => {})
    const validate = vi.fn(async () => ({ ok: false as const, errors }))
    const logFailure = vi.fn<(errors: DispatchPreflightError[]) => void>()

    const result = await runTickPreflightGate({
      reconcile,
      validate,
      logFailure,
    })

    expect(result).toEqual({ dispatchAllowed: false })
    expect(reconcile).toHaveBeenCalledTimes(1)
    expect(validate).toHaveBeenCalledTimes(1)
    expect(logFailure).toHaveBeenCalledTimes(1)
    expect(logFailure).toHaveBeenCalledWith(errors)
    expect(reconcile.mock.invocationCallOrder[0]).toBeLessThan(
      validate.mock.invocationCallOrder[0],
    )
  })
})
