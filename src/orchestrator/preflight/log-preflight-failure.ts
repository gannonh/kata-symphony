import type { Logger } from '../../observability/contracts.js'
import type { DispatchPreflightError } from './contracts.js'

type DispatchPreflightPhase = 'startup' | 'tick'

type PreflightLogContext = Record<string, unknown>

function readNonBlankString(value: unknown): string | undefined {
  if (typeof value !== 'string') {
    return undefined
  }

  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : undefined
}

export function logPreflightFailure(
  logger: Logger,
  phase: DispatchPreflightPhase,
  errors: DispatchPreflightError[],
  context?: PreflightLogContext,
): void {
  const safeWorkflowPath =
    readNonBlankString(context?.['workflowPath']) ??
    readNonBlankString(context?.['workflow_path'])

  logger.error('Dispatch preflight validation failed', {
    phase,
    error_codes: errors.map((error) => error.code),
    errors: errors.map((error) => ({
      code: error.code,
      field: error.field,
      source: error.source,
      message: error.message,
    })),
    ...(safeWorkflowPath === undefined
      ? {}
      : { workflow_path: safeWorkflowPath }),
  })
}
