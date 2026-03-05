import { describe, expect, it } from 'vitest'
import {
  TrackerIntegrationError,
  createLinearApiRequestError,
  createLinearApiStatusError,
  createLinearGraphQLErrorsError,
  createLinearUnknownPayloadError,
  createLinearMissingEndCursorError,
} from '../../src/tracker/index.js'

describe('tracker module contracts', () => {
  it('exports stable linear error constructors with deterministic messages/details', () => {
    const requestCause = new Error('request timeout')
    const graphqlErrors = [
      { message: 'boom', path: ['issues'] },
      { message: 'bad input', extensions: { code: 'BAD_USER_INPUT' } },
    ]

    const e1 = createLinearApiRequestError('request failed', requestCause)
    const e2 = createLinearApiStatusError(500, 'Internal Error')
    const e3 = createLinearGraphQLErrorsError(graphqlErrors)
    const e4 = createLinearUnknownPayloadError('missing data.issues')
    const e5 = createLinearMissingEndCursorError('candidates')

    expect(e1).toBeInstanceOf(TrackerIntegrationError)
    expect(e1.code).toBe('linear_api_request')
    expect(e1.message).toBe('request failed')
    expect(e1.cause).toBe(requestCause)
    expect(e1.details).toEqual({
      cause: {
        message: 'request timeout',
        name: 'Error',
        type: 'error',
      },
    })

    expect(e2.code).toBe('linear_api_status')
    expect(e2.message).toBe('Linear API status 500: Internal Error')
    expect(e2.details).toEqual({ status: 500, statusText: 'Internal Error' })

    expect(e3.code).toBe('linear_graphql_errors')
    expect(e3.message).toBe('Linear GraphQL returned errors')
    expect(e3.details).toEqual({ errors: graphqlErrors })

    expect(e4.code).toBe('linear_unknown_payload')
    expect(e4.message).toBe('Linear payload invalid: missing data.issues')
    expect(e4.details).toEqual({})

    expect(e5.code).toBe('linear_missing_end_cursor')
    expect(e5.message).toBe(
      'Missing endCursor while hasNextPage=true for candidates',
    )
    expect(e5.details).toEqual({})
  })

  it('keeps request error details serializable for non-error causes', () => {
    const e = createLinearApiRequestError('request failed', { retryable: true })

    expect(e.details).toEqual({
      cause: {
        type: 'Object',
      },
    })
  })
})
