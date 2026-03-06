# KAT-226 Linear Tracker Adapter and Normalization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement a Linear tracker adapter that satisfies SPEC Section 11 query semantics, normalization, pagination, empty-input behavior, and typed error mapping through the existing `TrackerClient` contract.

**Architecture:** Add a dedicated `src/tracker/linear/` implementation split into query definitions, HTTP transport, normalization, and client orchestration. Keep failures typed and deterministic so orchestrator integration can branch on stable error codes. Build the implementation test-first with fixture-driven unit tests for each layer and a client-level integration suite for pagination and deterministic behavior.

**Tech Stack:** TypeScript (Node 22), built-in `fetch`/`AbortController`, Vitest, pnpm.

---

**Skill refs for execution:** `@test-driven-development`, `@verification-before-completion`, `@systematic-debugging`

## Task 1: Scaffold tracker adapter contracts and error surface

**Files:**
- Create: `src/tracker/errors.ts`
- Create: `src/tracker/index.ts`
- Create: `src/tracker/linear/types.ts`
- Test: `tests/tracker/tracker-module-contracts.test.ts`

**Step 1: Write the failing test**

```ts
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
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/tracker/tracker-module-contracts.test.ts`
Expected: FAIL with module/export errors because tracker runtime module does not exist yet.

**Step 3: Write minimal implementation**

```ts
// src/tracker/errors.ts
export type TrackerErrorCode =
  | 'linear_api_request'
  | 'linear_api_status'
  | 'linear_graphql_errors'
  | 'linear_unknown_payload'
  | 'linear_missing_end_cursor'

export class TrackerIntegrationError extends Error {
  readonly code: TrackerErrorCode
  readonly details: Record<string, unknown>

  constructor(code: TrackerErrorCode, message: string, details: Record<string, unknown> = {}) {
    super(message)
    this.name = 'TrackerIntegrationError'
    this.code = code
    this.details = details
  }
}

export const createLinearApiRequestError = (message: string, cause?: unknown) =>
  new TrackerIntegrationError('linear_api_request', message, { cause })

export const createLinearApiStatusError = (status: number, statusText: string) =>
  new TrackerIntegrationError('linear_api_status', `Linear API status ${status}: ${statusText}`, {
    status,
    statusText,
  })

export const createLinearGraphQLErrorsError = (errors: unknown[]) =>
  new TrackerIntegrationError('linear_graphql_errors', 'Linear GraphQL returned errors', { errors })

export const createLinearUnknownPayloadError = (reason: string) =>
  new TrackerIntegrationError('linear_unknown_payload', `Linear payload invalid: ${reason}`)

export const createLinearMissingEndCursorError = (operation: string) =>
  new TrackerIntegrationError(
    'linear_missing_end_cursor',
    `Missing endCursor while hasNextPage=true for ${operation}`,
  )
```

```ts
// src/tracker/index.ts
export * from './errors.js'
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/tracker/tracker-module-contracts.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/tracker/tracker-module-contracts.test.ts src/tracker/errors.ts src/tracker/index.ts src/tracker/linear/types.ts
git commit -m "feat(tracker): scaffold linear adapter error contracts"
```

## Task 2: Implement Linear issue normalization contract

**Files:**
- Create: `src/tracker/linear/normalize.ts`
- Modify: `src/tracker/index.ts`
- Test: `tests/tracker/linear/normalize.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { normalizeLinearIssue } from '../../../src/tracker/index.js'

describe('linear normalization', () => {
  it('normalizes labels, blockers, priority and timestamps', () => {
    const issue = normalizeLinearIssue({
      id: 'lin-1',
      identifier: 'KAT-226',
      title: 'Tracker work',
      description: 'desc',
      priority: 2.7,
      state: { name: 'Todo' },
      branchName: 'feature/kat-226',
      url: 'https://linear.app/kata-sh/issue/KAT-226',
      labels: { nodes: [{ name: 'Area:Symphony' }, { name: 'TRACK:Integration' }] },
      issueRelations: {
        nodes: [
          {
            type: 'blocks',
            issue: { id: 'lin-2', identifier: 'KAT-223', state: { name: 'Done' } },
          },
          {
            type: 'related',
            issue: { id: 'lin-3', identifier: 'KAT-999', state: { name: 'Todo' } },
          },
        ],
      },
      createdAt: '2026-03-05T00:00:00.000Z',
      updatedAt: 'not-a-date',
    })

    expect(issue.labels).toEqual(['area:symphony', 'track:integration'])
    expect(issue.blocked_by).toEqual([{ id: 'lin-2', identifier: 'KAT-223', state: 'Done' }])
    expect(issue.priority).toBeNull()
    expect(issue.created_at).toBe('2026-03-05T00:00:00.000Z')
    expect(issue.updated_at).toBeNull()
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/tracker/linear/normalize.test.ts`
Expected: FAIL because normalization function is missing.

**Step 3: Write minimal implementation**

```ts
// src/tracker/linear/normalize.ts
import type { Issue } from '../../domain/models.js'
import { createLinearUnknownPayloadError } from '../errors.js'
import type { LinearIssueNode } from './types.js'

function parseIso(value: unknown): string | null {
  if (typeof value !== 'string') return null
  const ms = Date.parse(value)
  return Number.isNaN(ms) ? null : new Date(ms).toISOString()
}

function toPriority(value: unknown): number | null {
  return typeof value === 'number' && Number.isInteger(value) ? value : null
}

export function normalizeLinearIssue(node: LinearIssueNode): Issue {
  if (!node?.id || !node?.identifier || !node?.title || !node?.state?.name) {
    throw createLinearUnknownPayloadError('issue node missing required fields')
  }

  const labels = (node.labels?.nodes ?? [])
    .map((label) => (typeof label?.name === 'string' ? label.name.toLowerCase() : null))
    .filter((value): value is string => value !== null)

  const blockedBy = (node.issueRelations?.nodes ?? [])
    .filter((relation) => relation?.type === 'blocks')
    .map((relation) => ({
      id: relation.issue?.id ?? null,
      identifier: relation.issue?.identifier ?? null,
      state: relation.issue?.state?.name ?? null,
    }))

  return {
    id: node.id,
    identifier: node.identifier,
    title: node.title,
    description: typeof node.description === 'string' ? node.description : null,
    priority: toPriority(node.priority),
    state: node.state.name,
    branch_name: typeof node.branchName === 'string' ? node.branchName : null,
    url: typeof node.url === 'string' ? node.url : null,
    labels,
    blocked_by: blockedBy,
    created_at: parseIso(node.createdAt),
    updated_at: parseIso(node.updatedAt),
  }
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/tracker/linear/normalize.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/tracker/linear/normalize.test.ts src/tracker/linear/normalize.ts src/tracker/index.ts
git commit -m "feat(tracker): implement linear issue normalization contract"
```

## Task 3: Add Linear query definitions and variable builders

**Files:**
- Create: `src/tracker/linear/queries.ts`
- Modify: `src/tracker/linear/types.ts`
- Modify: `src/tracker/index.ts`
- Test: `tests/tracker/linear/queries.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import {
  LINEAR_CANDIDATES_QUERY,
  LINEAR_ISSUES_BY_IDS_QUERY,
  LINEAR_TERMINAL_QUERY,
  buildCandidatesVariables,
  buildIssuesByIdsVariables,
} from '../../../src/tracker/index.js'

describe('linear queries', () => {
  it('uses project slug and pagination fields for candidate query', () => {
    expect(LINEAR_CANDIDATES_QUERY).toContain('slugId')
    expect(LINEAR_CANDIDATES_QUERY).toContain('hasNextPage')
    expect(LINEAR_CANDIDATES_QUERY).toContain('endCursor')
  })

  it('uses GraphQL ID typing for issue refresh', () => {
    expect(LINEAR_ISSUES_BY_IDS_QUERY).toContain('$issueIds: [ID!]')
  })

  it('builds deterministic variables with page size 50 default', () => {
    expect(buildCandidatesVariables('proj', ['Todo'], null)).toEqual({
      projectSlug: 'proj',
      states: ['Todo'],
      first: 50,
      after: null,
    })
    expect(buildIssuesByIdsVariables(['a', 'b'])).toEqual({ issueIds: ['a', 'b'] })
  })

  it('builds terminal-state variables by reusing candidate semantics', () => {
    expect(buildCandidatesVariables('proj', ['Done'], 'cursor-1')).toEqual({
      projectSlug: 'proj',
      states: ['Done'],
      first: 50,
      after: 'cursor-1',
    })
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/tracker/linear/queries.test.ts`
Expected: FAIL because queries/builders are missing.

**Step 3: Write minimal implementation**

```ts
// src/tracker/linear/queries.ts
export const LINEAR_CANDIDATES_QUERY = `
query Candidates($projectSlug: String!, $states: [String!]!, $first: Int!, $after: String) {
  issues(
    filter: {
      project: { slugId: { eq: $projectSlug } }
      state: { name: { in: $states } }
    }
    first: $first
    after: $after
  ) {
    nodes { ...IssueFields }
    pageInfo { hasNextPage endCursor }
  }
}
fragment IssueFields on Issue {
  id identifier title description priority branchName url createdAt updatedAt
  state { name }
  labels { nodes { name } }
  issueRelations { nodes { type issue { id identifier state { name } } } }
}
`

export const LINEAR_TERMINAL_QUERY = LINEAR_CANDIDATES_QUERY

export const LINEAR_ISSUES_BY_IDS_QUERY = `
query IssuesByIds($issueIds: [ID!]) {
  issues(filter: { id: { in: $issueIds } }) {
    nodes { ...IssueFields }
  }
}
fragment IssueFields on Issue {
  id identifier title description priority branchName url createdAt updatedAt
  state { name }
  labels { nodes { name } }
  issueRelations { nodes { type issue { id identifier state { name } } } }
}
`

export function buildCandidatesVariables(projectSlug: string, states: string[], after: string | null) {
  return { projectSlug, states, first: 50, after }
}

export function buildIssuesByIdsVariables(issueIds: string[]) {
  return { issueIds }
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/tracker/linear/queries.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/tracker/linear/queries.test.ts src/tracker/linear/queries.ts src/tracker/linear/types.ts src/tracker/index.ts
git commit -m "feat(tracker): add linear graphql query contract"
```

## Task 4: Implement GraphQL transport with timeout and typed error mapping

**Files:**
- Create: `src/tracker/linear/http.ts`
- Modify: `src/tracker/index.ts`
- Test: `tests/tracker/linear/http.test.ts`

**Step 1: Write the failing test**

```ts
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
    const response = new Response(JSON.stringify({}), { status: 503, statusText: 'Service Unavailable' })
    const fetchImpl = vi.fn(async () => response)

    await expect(
      runLinearGraphQL({ endpoint: 'x', apiKey: 'y', query: 'q', variables: {}, fetchImpl }),
    ).rejects.toMatchObject({ code: 'linear_api_status' })
  })

  it('maps graphql errors to linear_graphql_errors', async () => {
    const response = new Response(JSON.stringify({ errors: [{ message: 'bad query' }] }), { status: 200 })
    const fetchImpl = vi.fn(async () => response)

    await expect(
      runLinearGraphQL({ endpoint: 'x', apiKey: 'y', query: 'q', variables: {}, fetchImpl }),
    ).rejects.toMatchObject({ code: 'linear_graphql_errors' })
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/tracker/linear/http.test.ts`
Expected: FAIL because transport helper is missing.

**Step 3: Write minimal implementation**

```ts
// src/tracker/linear/http.ts
import {
  createLinearApiRequestError,
  createLinearApiStatusError,
  createLinearGraphQLErrorsError,
} from '../errors.js'

interface RunLinearGraphQLInput {
  endpoint: string
  apiKey: string
  query: string
  variables: Record<string, unknown>
  timeoutMs?: number
  fetchImpl?: typeof fetch
}

export async function runLinearGraphQL(input: RunLinearGraphQLInput): Promise<unknown> {
  const controller = new AbortController()
  const timeout = setTimeout(() => controller.abort(), input.timeoutMs ?? 30_000)
  const fetchImpl = input.fetchImpl ?? fetch

  try {
    const response = await fetchImpl(input.endpoint, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        authorization: input.apiKey,
      },
      body: JSON.stringify({ query: input.query, variables: input.variables }),
      signal: controller.signal,
    })

    if (!response.ok) {
      throw createLinearApiStatusError(response.status, response.statusText)
    }

    const payload = (await response.json()) as { data?: unknown; errors?: unknown[] }
    if (Array.isArray(payload.errors) && payload.errors.length > 0) {
      throw createLinearGraphQLErrorsError(payload.errors)
    }

    return payload.data
  } catch (error) {
    if (error instanceof Error && 'code' in error) {
      throw error
    }
    throw createLinearApiRequestError('Linear request failed', error)
  } finally {
    clearTimeout(timeout)
  }
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/tracker/linear/http.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/tracker/linear/http.test.ts src/tracker/linear/http.ts src/tracker/index.ts
git commit -m "feat(tracker): add linear graphql transport error mapping"
```

## Task 5: Implement `TrackerClient` with pagination and deterministic behavior

**Files:**
- Create: `src/tracker/linear/client.ts`
- Modify: `src/tracker/index.ts`
- Modify: `src/tracker/contracts.ts` (type-only helper exports only if needed; do not change method signatures)
- Test: `tests/tracker/linear/client.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it, vi } from 'vitest'
import { createStaticConfigProvider } from '../../../src/config/contracts.js'
import { createLinearTrackerClient } from '../../../src/tracker/index.js'

describe('linear tracker client', () => {
  it('returns [] for fetchIssuesByIds([]) without API call', async () => {
    const fetchImpl = vi.fn()
    const client = createLinearTrackerClient(
      createStaticConfigProvider({
        tracker: {
          kind: 'linear',
          endpoint: 'https://api.linear.app/graphql',
          api_key: 'token',
          project_slug: 'proj',
          active_states: ['Todo'],
          terminal_states: ['Done'],
        },
      }),
      fetchImpl,
    )

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
                nodes: [{ id: '1', identifier: 'KAT-1', title: 'a', state: { name: 'Todo' }, labels: { nodes: [] }, issueRelations: { nodes: [] } }],
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
                nodes: [{ id: '2', identifier: 'KAT-2', title: 'b', state: { name: 'Todo' }, labels: { nodes: [] }, issueRelations: { nodes: [] } }],
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
    await expect(client.fetchCandidates()).rejects.toMatchObject({ code: 'linear_missing_end_cursor' })
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/tracker/linear/client.test.ts`
Expected: FAIL because client implementation is missing.

**Step 3: Write minimal implementation**

```ts
// src/tracker/linear/client.ts
import type { ConfigProvider } from '../../config/contracts.js'
import type { Issue } from '../../domain/models.js'
import type { TrackerClient } from '../contracts.js'
import { createLinearMissingEndCursorError, createLinearUnknownPayloadError } from '../errors.js'
import { runLinearGraphQL } from './http.js'
import { normalizeLinearIssue } from './normalize.js'
import {
  LINEAR_CANDIDATES_QUERY,
  LINEAR_ISSUES_BY_IDS_QUERY,
  LINEAR_TERMINAL_QUERY,
  buildCandidatesVariables,
  buildIssuesByIdsVariables,
} from './queries.js'
import type { LinearIssueConnection } from './types.js'

function readIssueConnection(data: unknown): LinearIssueConnection {
  const connection = (data as { issues?: unknown })?.issues
  if (!connection || typeof connection !== 'object') {
    throw createLinearUnknownPayloadError('missing issues connection')
  }
  return connection as LinearIssueConnection
}

export function createLinearTrackerClient(config: ConfigProvider, fetchImpl?: typeof fetch): TrackerClient {
  async function fetchByStates(states: string[], query: string, operation: string): Promise<Issue[]> {
    const snapshot = config.getSnapshot()
    const result: Issue[] = []
    let cursor: string | null = null

    while (true) {
      const data = await runLinearGraphQL({
        endpoint: snapshot.tracker.endpoint,
        apiKey: snapshot.tracker.api_key,
        query,
        variables: buildCandidatesVariables(snapshot.tracker.project_slug, states, cursor),
        timeoutMs: 30_000,
        fetchImpl,
      })

      const connection = readIssueConnection(data)
      const nodes = Array.isArray(connection.nodes) ? connection.nodes : []
      result.push(...nodes.map((node) => normalizeLinearIssue(node)))

      const hasNextPage = connection.pageInfo?.hasNextPage === true
      const endCursor = connection.pageInfo?.endCursor ?? null
      if (!hasNextPage) break
      if (endCursor === null) throw createLinearMissingEndCursorError(operation)
      cursor = endCursor
    }

    return result
  }

  return {
    async fetchCandidates() {
      const snapshot = config.getSnapshot()
      return fetchByStates(snapshot.tracker.active_states, LINEAR_CANDIDATES_QUERY, 'fetchCandidates')
    },

    async fetchIssuesByIds(issueIds: string[]) {
      if (issueIds.length === 0) return []

      const snapshot = config.getSnapshot()
      const data = await runLinearGraphQL({
        endpoint: snapshot.tracker.endpoint,
        apiKey: snapshot.tracker.api_key,
        query: LINEAR_ISSUES_BY_IDS_QUERY,
        variables: buildIssuesByIdsVariables(issueIds),
        timeoutMs: 30_000,
        fetchImpl,
      })

      const connection = readIssueConnection(data)
      const nodes = Array.isArray(connection.nodes) ? connection.nodes : []
      return nodes.map((node) => normalizeLinearIssue(node))
    },

    async fetchTerminalIssues() {
      const snapshot = config.getSnapshot()
      return fetchByStates(snapshot.tracker.terminal_states, LINEAR_TERMINAL_QUERY, 'fetchTerminalIssues')
    },
  }
}
```

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/tracker/linear/client.test.ts`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/tracker/linear/client.test.ts src/tracker/linear/client.ts src/tracker/index.ts src/tracker/contracts.ts
git commit -m "feat(tracker): implement linear tracker client pagination and refresh"
```

## Task 6: Add integration contract tests and architecture touchpoint

**Files:**
- Modify: `tests/contracts/layer-contracts.test.ts`
- Modify: `tests/contracts/runtime-modules.test.ts`
- Modify: `ARCHITECTURE.md`

**Step 1: Write the failing test**

```ts
import { createStaticConfigProvider } from '../../src/config/contracts.js'
import { createLinearTrackerClient } from '../../src/tracker/index.js'

it('builds a concrete linear tracker adapter from config layer', async () => {
  const client = createLinearTrackerClient(
    createStaticConfigProvider({
      tracker: {
        kind: 'linear',
        endpoint: 'https://api.linear.app/graphql',
        api_key: 'token',
        project_slug: 'proj',
        active_states: ['Todo'],
        terminal_states: ['Done'],
      },
    }),
    async () =>
      new Response(
        JSON.stringify({
          data: {
            issues: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
          },
        }),
      ),
  )

  await expect(client.fetchCandidates()).resolves.toEqual([])
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest run tests/contracts/layer-contracts.test.ts tests/contracts/runtime-modules.test.ts`
Expected: FAIL until tracker runtime exports are added to expectations.

**Step 3: Write minimal implementation**

- Update contract tests to assert runtime tracker module exports include:
  - `createLinearTrackerClient`
  - `TrackerIntegrationError`
- Update `ARCHITECTURE.md` tracker module map to include `src/tracker/linear/*` and `src/tracker/errors.ts`.

**Step 4: Run test to verify it passes**

Run: `pnpm vitest run tests/contracts/layer-contracts.test.ts tests/contracts/runtime-modules.test.ts tests/tracker`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/contracts/layer-contracts.test.ts tests/contracts/runtime-modules.test.ts ARCHITECTURE.md
git commit -m "test(tracker): wire runtime tracker contract coverage"
```

## Task 7: Full verification gate before handoff

**Files:**
- Modify: `docs/generated/kat-226-verification.md` (create only if your PR evidence flow uses it)

**Step 1: Run tracker-focused test suites**

Run: `pnpm vitest run tests/tracker tests/contracts`
Expected: PASS.

**Step 2: Run repository quality gates**

Run: `pnpm run lint && pnpm run typecheck && pnpm test`
Expected: PASS.

**Step 3: Run harness gate**

Run: `make check`
Expected: PASS.

**Step 4: Record evidence and AC mapping**

Capture evidence for `KAT-226`:
- Query semantics match Section 11.2 (project slug filter, `[ID!]`, pagination size 50, timeout 30000ms).
- Error mapping matches Section 11.4 categories.
- Normalized issue model supports dispatch/reconciliation fields and deterministic pagination/empty-input behavior.

**Step 5: Commit evidence docs (if created)**

```bash
git add docs/generated/kat-226-verification.md
git commit -m "docs: add KAT-226 verification evidence"
```
