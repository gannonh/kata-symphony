import type { DispatchPreflightError, DispatchPreflightResult } from './contracts.js'

export interface RunTickPreflightGateOptions {
  reconcile: () => Promise<void>
  validate: () => Promise<DispatchPreflightResult>
  logFailure: (errors: DispatchPreflightError[]) => void
}

export interface RunTickPreflightGateResult {
  dispatchAllowed: boolean
}

export async function runTickPreflightGate(
  options: RunTickPreflightGateOptions,
): Promise<RunTickPreflightGateResult> {
  await options.reconcile()

  const result = await options.validate()
  if (result.ok === false) {
    options.logFailure(result.errors)
    return { dispatchAllowed: false }
  }

  return { dispatchAllowed: true }
}
