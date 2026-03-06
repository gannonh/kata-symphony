import { describe, expect, it } from 'vitest'
import {
  LINEAR_CANDIDATES_QUERY,
  LINEAR_ISSUES_BY_IDS_QUERY,
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
