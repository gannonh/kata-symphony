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
  ) {
    super(message)
    this.name = 'TrackerIntegrationError'
    this.code = code
    this.details = details
  }
}

export function createLinearApiRequestError(
  message: string,
  cause?: unknown,
): TrackerIntegrationError {
  return new TrackerIntegrationError('linear_api_request', message, { cause })
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
  errors: unknown[],
): TrackerIntegrationError {
  return new TrackerIntegrationError(
    'linear_graphql_errors',
    'Linear GraphQL returned errors',
    { errors },
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
