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

  it('uses global fetch when fetchImpl is omitted', async () => {
    const originalFetch = globalThis.fetch
    const fetchMock = vi.fn(async () => {
      return new Response(JSON.stringify({ data: { viewer: { id: 'v1' } } }), {
        status: 200,
      })
    })
    globalThis.fetch = fetchMock as typeof fetch

    try {
      await expect(
        runLinearGraphQL({
          endpoint: 'https://api.linear.app/graphql',
          apiKey: 'secret',
          query: 'query { viewer { id } }',
          variables: {},
        }),
      ).resolves.toEqual({ viewer: { id: 'v1' } })
      expect(fetchMock).toHaveBeenCalledTimes(1)
    } finally {
      globalThis.fetch = originalFetch
    }
  })

  it('sends authorization header with Bearer prefix', async () => {
    let capturedHeaders: HeadersInit | undefined
    const fetchImpl = vi.fn(async (_url: string | URL | Request, init?: RequestInit) => {
      capturedHeaders = init?.headers
      return new Response(JSON.stringify({ data: { ok: true } }), { status: 200 })
    })

    await runLinearGraphQL({
      endpoint: 'https://api.linear.app/graphql',
      apiKey: 'test-token',
      query: 'query { viewer { id } }',
      variables: {},
      fetchImpl,
    })

    expect(capturedHeaders).toMatchObject({
      authorization: 'Bearer test-token',
    })
  })

  it('maps null response body to linear_unknown_payload', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response('null', { status: 200, headers: { 'content-type': 'application/json' } }),
    )

    await expect(
      runLinearGraphQL({
        endpoint: 'x',
        apiKey: 'y',
        query: 'q',
        variables: {},
        fetchImpl,
      }),
    ).rejects.toMatchObject({ code: 'linear_unknown_payload' })
  })

  it('maps array response body to linear_unknown_payload', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response('[]', { status: 200, headers: { 'content-type': 'application/json' } }),
    )

    await expect(
      runLinearGraphQL({
        endpoint: 'x',
        apiKey: 'y',
        query: 'q',
        variables: {},
        fetchImpl,
      }),
    ).rejects.toMatchObject({ code: 'linear_unknown_payload' })
  })

  it('treats empty graphql errors array as non-failure payload', async () => {
    const response = new Response(
      JSON.stringify({ data: { ok: true }, errors: [] }),
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
    ).resolves.toEqual({ ok: true })
  })
})
