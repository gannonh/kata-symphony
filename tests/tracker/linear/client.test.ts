import { describe, expect, it, vi } from 'vitest'
import { createStaticConfigProvider } from '../../../src/config/contracts.js'
import { createLinearTrackerClient } from '../../../src/tracker/index.js'

function testConfig() {
  return createStaticConfigProvider({
    tracker: {
      kind: 'linear',
      endpoint: 'https://api.linear.app/graphql',
      api_key: 'token',
      project_slug: 'proj',
      active_states: ['Todo'],
      terminal_states: ['Done'],
    },
  })
}

describe('linear tracker client', () => {
  it('returns [] for fetchIssuesByIds([]) without API call', async () => {
    const fetchImpl = vi.fn()
    const client = createLinearTrackerClient(testConfig(), fetchImpl)

    await expect(client.fetchIssuesByIds([])).resolves.toEqual([])
    expect(fetchImpl).not.toHaveBeenCalled()
  })

  it('paginates candidates preserving order', async () => {
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            data: {
              issues: {
                nodes: [
                  {
                    id: '1',
                    identifier: 'KAT-1',
                    title: 'a',
                    state: { name: 'Todo' },
                    labels: { nodes: [] },
                    inverseRelations: { nodes: [] },
                  },
                ],
                pageInfo: { hasNextPage: true, endCursor: 'c1' },
              },
            },
          }),
        ),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            data: {
              issues: {
                nodes: [
                  {
                    id: '2',
                    identifier: 'KAT-2',
                    title: 'b',
                    state: { name: 'Todo' },
                    labels: { nodes: [] },
                    inverseRelations: { nodes: [] },
                  },
                ],
                pageInfo: { hasNextPage: false, endCursor: null },
              },
            },
          }),
        ),
      )

    const client = createLinearTrackerClient(testConfig(), fetchImpl)
    const result = await client.fetchCandidates()

    expect(result.map((issue) => issue.id)).toEqual(['1', '2'])
  })

  it('throws linear_missing_end_cursor when hasNextPage=true and cursor missing', async () => {
    const fetchImpl = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          data: {
            issues: {
              nodes: [],
              pageInfo: { hasNextPage: true, endCursor: null },
            },
          },
        }),
      ),
    )

    const client = createLinearTrackerClient(testConfig(), fetchImpl)
    await expect(client.fetchCandidates()).rejects.toMatchObject({
      code: 'linear_missing_end_cursor',
    })
  })

  it('fetches issues by ids when ids are present', async () => {
    const fetchImpl = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          data: {
            issues: {
              nodes: [
                {
                  id: '11',
                  identifier: 'KAT-11',
                  title: 'refresh',
                  state: { name: 'Todo' },
                  labels: { nodes: [] },
                  inverseRelations: { nodes: [] },
                },
              ],
            },
          },
        }),
      ),
    )
    const client = createLinearTrackerClient(testConfig(), fetchImpl)

    await expect(client.fetchIssuesByIds(['11'])).resolves.toMatchObject([
      { id: '11', identifier: 'KAT-11' },
    ])
    expect(fetchImpl).toHaveBeenCalledTimes(1)
  })

  it('fetches terminal issues using global fetch when no fetch impl is provided', async () => {
    const originalFetch = globalThis.fetch
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          data: {
            issues: {
              nodes: [
                {
                  id: '20',
                  identifier: 'KAT-20',
                  title: 'done',
                  state: { name: 'Done' },
                  labels: { nodes: [] },
                  inverseRelations: { nodes: [] },
                },
              ],
              pageInfo: { hasNextPage: false, endCursor: null },
            },
          },
        }),
      ),
    )
    globalThis.fetch = fetchMock as typeof fetch

    try {
      const client = createLinearTrackerClient(testConfig())
      await expect(client.fetchTerminalIssues()).resolves.toMatchObject([
        { id: '20', state: 'Done' },
      ])
      expect(fetchMock).toHaveBeenCalledTimes(1)
    } finally {
      globalThis.fetch = originalFetch
    }
  })

  it('maps malformed payloads to linear_unknown_payload', async () => {
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({ data: null })))
      .mockResolvedValueOnce(new Response(JSON.stringify({ data: {} })))

    const client = createLinearTrackerClient(testConfig(), fetchImpl)

    await expect(client.fetchCandidates()).rejects.toMatchObject({
      code: 'linear_unknown_payload',
    })
    await expect(client.fetchIssuesByIds(['KAT-1'])).rejects.toMatchObject({
      code: 'linear_unknown_payload',
    })
  })

  it('treats missing nodes arrays as empty result sets', async () => {
    const fetchImpl = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            data: {
              issues: {
                pageInfo: { hasNextPage: false, endCursor: null },
              },
            },
          }),
        ),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            data: {
              issues: {},
            },
          }),
        ),
      )

    const client = createLinearTrackerClient(testConfig(), fetchImpl)

    await expect(client.fetchCandidates()).resolves.toEqual([])
    await expect(client.fetchIssuesByIds(['x'])).resolves.toEqual([])
  })
})
