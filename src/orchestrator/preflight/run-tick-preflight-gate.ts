import type { DispatchPreflightError, DispatchPreflightResult } from './contracts.js'

export interface RunTickPreflightGateOptions {
  validate: () => Promise<DispatchPreflightResult>
  logFailure: (errors: DispatchPreflightError[]) => void | Promise<void>
}

export interface RunTickPreflightGateResult {
  dispatchAllowed: boolean
}

const VALIDATE_THROW_FAILURE: DispatchPreflightError[] = [
  {
    code: 'workflow_invalid',
    source: 'workflow',
    field: 'workflow',
    message: 'Workflow file cannot be loaded or parsed',
  },
]

export async function runTickPreflightGate(
  options: RunTickPreflightGateOptions,
): Promise<RunTickPreflightGateResult> {
  let result: DispatchPreflightResult
  try {
    result = await options.validate()
  } catch (error) {
    void error
    result = { ok: false, errors: VALIDATE_THROW_FAILURE }
  }

  if (result.ok === false) {
    try {
      await options.logFailure(result.errors)
    } catch (error) {
      void error
    }
    return { dispatchAllowed: false }
  }

  return { dispatchAllowed: true }
}
