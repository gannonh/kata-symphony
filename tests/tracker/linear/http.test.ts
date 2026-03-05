import { describe, expect, it, vi } from 'vitest'
import { runLinearGraphQL } from '../../../src/tracker/index.js'

describe('linear http transport', () => {
  it('maps fetch rejection to linear_api_request', async () => {
    const fetchImpl = vi.fn(async () => {
      throw new Error('connect ECONNREFUSED')
    })

    await expect(
      runLinearGraphQL({
        endpoint: 'https://api.linear.app/graphql',
        apiKey: 'secret',
        query: 'query { viewer { id } }',
        variables: {},
        fetchImpl,
      }),
    ).rejects.toMatchObject({ code: 'linear_api_request' })
  })

  it('maps non-200 status to linear_api_status', async () => {
    const response = new Response(JSON.stringify({}), {
      status: 503,
      statusText: 'Service Unavailable',
    })
    const fetchImpl = vi.fn(async () => response)

    await expect(
      runLinearGraphQL({
        endpoint: 'x',
        apiKey: 'y',
        query: 'q',
        variables: {},
        fetchImpl,
      }),
    ).rejects.toMatchObject({ code: 'linear_api_status' })
  })

  it('maps graphql errors to linear_graphql_errors', async () => {
    const response = new Response(
      JSON.stringify({ errors: [{ message: 'bad query' }] }),
      { status: 200 },
    )
    const fetchImpl = vi.fn(async () => response)

    await expect(
      runLinearGraphQL({
        endpoint: 'x',
        apiKey: 'y',
        query: 'q',
        variables: {},
        fetchImpl,
      }),
    ).rejects.toMatchObject({ code: 'linear_graphql_errors' })
  })
})
