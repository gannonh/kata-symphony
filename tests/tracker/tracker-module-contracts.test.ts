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
  it('exports stable linear error constructors', () => {
    const e1 = createLinearApiRequestError('request failed')
    const e2 = createLinearApiStatusError(500, 'Internal Error')
    const e3 = createLinearGraphQLErrorsError([{ message: 'boom' }])
    const e4 = createLinearUnknownPayloadError('missing data.issues')
    const e5 = createLinearMissingEndCursorError('candidates')

    expect(e1).toBeInstanceOf(TrackerIntegrationError)
    expect([e1.code, e2.code, e3.code, e4.code, e5.code]).toEqual([
      'linear_api_request',
      'linear_api_status',
      'linear_graphql_errors',
      'linear_unknown_payload',
      'linear_missing_end_cursor',
    ])
  })
})
