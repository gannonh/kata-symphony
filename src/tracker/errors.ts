import type { LinearGraphQLError } from './linear/types.js'

export type TrackerErrorCode =
  | 'linear_api_request'
  | 'linear_api_status'
  | 'linear_graphql_errors'
  | 'linear_unknown_payload'
  | 'linear_missing_end_cursor'

export class TrackerIntegrationError extends Error {
  readonly code: TrackerErrorCode
  readonly details: Record<string, unknown>

  constructor(
    code: TrackerErrorCode,
    message: string,
    details: Record<string, unknown> = {},
    cause?: unknown,
  ) {
    super(message, cause === undefined ? undefined : { cause })
    this.name = 'TrackerIntegrationError'
    this.code = code
    this.details = { ...details }
  }
}

function serializeCauseDetail(cause: unknown): Record<string, unknown> | undefined {
  if (cause === undefined) {
    return undefined
  }

  if (cause instanceof Error) {
    return {
      type: 'error',
      name: cause.name,
      message: cause.message,
    }
  }

  if (
    typeof cause === 'string' ||
    typeof cause === 'number' ||
    typeof cause === 'boolean' ||
    cause === null
  ) {
    return {
      type: typeof cause,
      value: cause,
    }
  }

  return {
    type: Object.prototype.toString.call(cause).slice(8, -1),
  }
}

export function createLinearApiRequestError(
  message: string,
  cause?: unknown,
): TrackerIntegrationError {
  const serializedCause = serializeCauseDetail(cause)
  return new TrackerIntegrationError(
    'linear_api_request',
    message,
    serializedCause === undefined ? {} : { cause: serializedCause },
    cause,
  )
}

export function createLinearApiStatusError(
  status: number,
  statusText: string,
): TrackerIntegrationError {
  return new TrackerIntegrationError(
    'linear_api_status',
    `Linear API status ${status}: ${statusText}`,
    { status, statusText },
  )
}

export function createLinearGraphQLErrorsError(
  errors: LinearGraphQLError[],
): TrackerIntegrationError {
  return new TrackerIntegrationError(
    'linear_graphql_errors',
    'Linear GraphQL returned errors',
    { errors: [...errors] },
  )
}

export function createLinearUnknownPayloadError(
  reason: string,
): TrackerIntegrationError {
  return new TrackerIntegrationError(
    'linear_unknown_payload',
    `Linear payload invalid: ${reason}`,
  )
}

export function createLinearMissingEndCursorError(
  operation: string,
): TrackerIntegrationError {
  return new TrackerIntegrationError(
    'linear_missing_end_cursor',
    `Missing endCursor while hasNextPage=true for ${operation}`,
  )
}
