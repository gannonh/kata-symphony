import type { ConfigProvider } from '../../config/contracts.js'
import type { Issue } from '../../domain/models.js'
import type { TrackerClient } from '../contracts.js'
import {
  createLinearMissingEndCursorError,
  createLinearUnknownPayloadError,
} from '../errors.js'
import { runLinearGraphQL } from './http.js'
import { normalizeLinearIssue } from './normalize.js'
import {
  LINEAR_CANDIDATES_QUERY,
  LINEAR_ISSUES_BY_IDS_QUERY,
  LINEAR_TERMINAL_QUERY,
  buildCandidatesVariables,
  buildIssuesByIdsVariables,
} from './queries.js'
import type { RunLinearGraphQLInput } from './http.js'
import type { LinearIssueConnection } from './types.js'

function readIssueConnection(data: unknown): LinearIssueConnection {
  if (!data || typeof data !== 'object') {
    throw createLinearUnknownPayloadError('missing issues connection')
  }

  const connection = (data as { issues?: unknown }).issues
  if (!connection || typeof connection !== 'object') {
    throw createLinearUnknownPayloadError('missing issues connection')
  }

  return connection as LinearIssueConnection
}

function withOptionalFetch(
  input: Omit<RunLinearGraphQLInput, 'fetchImpl'>,
  fetchImpl?: typeof fetch,
): RunLinearGraphQLInput {
  if (fetchImpl === undefined) {
    return input
  }

  return {
    ...input,
    fetchImpl,
  }
}

export function createLinearTrackerClient(
  config: ConfigProvider,
  fetchImpl?: typeof fetch,
): TrackerClient {
  async function fetchByStates(
    states: string[],
    query: string,
    operation: string,
  ): Promise<Issue[]> {
    const snapshot = config.getSnapshot()
    const result: Issue[] = []
    let after: string | null = null

    while (true) {
      const data = await runLinearGraphQL(
        withOptionalFetch(
          {
            endpoint: snapshot.tracker.endpoint,
            apiKey: snapshot.tracker.api_key,
            query,
            variables: buildCandidatesVariables(
              snapshot.tracker.project_slug,
              states,
              after,
            ),
            timeoutMs: 30_000,
          },
          fetchImpl,
        ),
      )

      const connection = readIssueConnection(data)
      const nodes = Array.isArray(connection.nodes) ? connection.nodes : []
      result.push(...nodes.map((node) => normalizeLinearIssue(node)))

      const hasNextPage = connection.pageInfo?.hasNextPage === true
      const endCursor = connection.pageInfo?.endCursor ?? null
      if (!hasNextPage) {
        break
      }

      if (endCursor === null) {
        throw createLinearMissingEndCursorError(operation)
      }

      after = endCursor
    }

    return result
  }

  return {
    async fetchCandidates(): Promise<Issue[]> {
      const snapshot = config.getSnapshot()
      return fetchByStates(
        snapshot.tracker.active_states,
        LINEAR_CANDIDATES_QUERY,
        'fetchCandidates',
      )
    },

    async fetchIssuesByIds(issueIds: string[]): Promise<Issue[]> {
      if (issueIds.length === 0) {
        return []
      }

      if (issueIds.length > 50) {
        throw createLinearUnknownPayloadError(
          `fetchIssuesByIds: ${issueIds.length} IDs exceeds single-page limit of 50`,
        )
      }

      const snapshot = config.getSnapshot()
      const data = await runLinearGraphQL(
        withOptionalFetch(
          {
            endpoint: snapshot.tracker.endpoint,
            apiKey: snapshot.tracker.api_key,
            query: LINEAR_ISSUES_BY_IDS_QUERY,
            variables: buildIssuesByIdsVariables(issueIds),
            timeoutMs: 30_000,
          },
          fetchImpl,
        ),
      )

      const connection = readIssueConnection(data)
      const nodes = Array.isArray(connection.nodes) ? connection.nodes : []
      return nodes.map((node) => normalizeLinearIssue(node))
    },

    async fetchTerminalIssues(): Promise<Issue[]> {
      const snapshot = config.getSnapshot()
      return fetchByStates(
        snapshot.tracker.terminal_states,
        LINEAR_TERMINAL_QUERY,
        'fetchTerminalIssues',
      )
    },
  }
}
