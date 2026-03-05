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
})
