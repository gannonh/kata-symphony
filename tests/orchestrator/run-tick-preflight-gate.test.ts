import { describe, expect, it, vi } from 'vitest'
import type { DispatchPreflightError } from '../../src/orchestrator/preflight/contracts.js'
import { runTickPreflightGate } from '../../src/orchestrator/preflight/index.js'

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T | PromiseLike<T>) => void
}

function createDeferred<T>(): Deferred<T> {
  let resolve: Deferred<T>['resolve'] = () => {}
  const promise = new Promise<T>((innerResolve) => {
    resolve = innerResolve
  })

  return {
    promise,
    resolve,
  }
}

describe('runTickPreflightGate', () => {
  it('awaits reconcile before validate and blocks dispatch on preflight failure', async () => {
    const errors: DispatchPreflightError[] = [
      {
        code: 'workflow_invalid',
        source: 'workflow',
        field: 'workflow',
        message: 'Workflow file cannot be loaded or parsed',
      },
    ]

    const reconcileDeferred = createDeferred<void>()
    const logFailureDeferred = createDeferred<void>()

    const reconcile = vi.fn(async () => {
      await reconcileDeferred.promise
    })
    const validate = vi.fn(async () => ({ ok: false as const, errors }))
    const logFailure = vi.fn(async (failureErrors: DispatchPreflightError[]) => {
      expect(failureErrors).toEqual(errors)
      await logFailureDeferred.promise
    })

    const gatePromise = runTickPreflightGate({
      reconcile,
      validate,
      logFailure,
    })

    await Promise.resolve()
    expect(validate).not.toHaveBeenCalled()

    reconcileDeferred.resolve()
    await vi.waitFor(() => {
      expect(validate).toHaveBeenCalledTimes(1)
      expect(logFailure).toHaveBeenCalledTimes(1)
    })

    let settled = false
    gatePromise.then(() => {
      settled = true
    })
    await Promise.resolve()
    expect(settled).toBe(false)

    logFailureDeferred.resolve()
    const result = await gatePromise

    expect(result).toEqual({ dispatchAllowed: false })
    expect(reconcile).toHaveBeenCalledTimes(1)
    expect(validate).toHaveBeenCalledTimes(1)
    expect(logFailure).toHaveBeenCalledTimes(1)
    expect(logFailure).toHaveBeenCalledWith(errors)
  })

  it('allows dispatch when validation passes', async () => {
    const reconcile = vi.fn(async () => {})
    const validate = vi.fn(async () => ({ ok: true as const }))
    const logFailure = vi.fn<(errors: DispatchPreflightError[]) => void>()

    const result = await runTickPreflightGate({
      reconcile,
      validate,
      logFailure,
    })

    expect(result).toEqual({ dispatchAllowed: true })
    expect(reconcile).toHaveBeenCalledTimes(1)
    expect(validate).toHaveBeenCalledTimes(1)
    expect(logFailure).not.toHaveBeenCalled()
  })

  it('blocks dispatch when validate throws', async () => {
    const reconcile = vi.fn(async () => {})
    const validate = vi.fn(async () => {
      throw new Error('validate failed')
    })
    const logFailure = vi.fn(async (errors: DispatchPreflightError[]) => {
      expect(errors).toEqual([
        {
          code: 'workflow_invalid',
          source: 'workflow',
          field: 'workflow',
          message: 'Workflow file cannot be loaded or parsed',
        },
      ])
    })

    const result = await runTickPreflightGate({
      reconcile,
      validate,
      logFailure,
    })

    expect(result).toEqual({ dispatchAllowed: false })
    expect(reconcile).toHaveBeenCalledTimes(1)
    expect(validate).toHaveBeenCalledTimes(1)
    expect(logFailure).toHaveBeenCalledTimes(1)
  })

  it('blocks dispatch when logFailure rejects', async () => {
    const errors: DispatchPreflightError[] = [
      {
        code: 'tracker_api_key_missing',
        source: 'config',
        field: 'tracker.api_key',
        message: 'tracker.api_key is required after resolution',
      },
    ]

    const reconcile = vi.fn(async () => {})
    const validate = vi.fn(async () => ({ ok: false as const, errors }))
    const logFailure = vi.fn(async () => {
      throw new Error('log failure broke')
    })

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
  })
})
