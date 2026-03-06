const LINEAR_ISSUE_FIELDS_FRAGMENT = `
fragment IssueFields on Issue {
  id
  identifier
  title
  description
  priority
  branchName
  url
  createdAt
  updatedAt
  state { name }
  labels { nodes { name } }
  inverseRelations { nodes { type issue { id identifier state { name } } } }
}
`

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
${LINEAR_ISSUE_FIELDS_FRAGMENT}
`

export const LINEAR_TERMINAL_QUERY = LINEAR_CANDIDATES_QUERY

export const LINEAR_ISSUES_BY_IDS_QUERY = `
query IssuesByIds($issueIds: [ID!]!) {
  issues(filter: { id: { in: $issueIds } }) {
    nodes { ...IssueFields }
  }
}
${LINEAR_ISSUE_FIELDS_FRAGMENT}
`

export function buildCandidatesVariables(
  projectSlug: string,
  states: string[],
  after: string | null,
) {
  return {
    projectSlug,
    states,
    first: 50,
    after,
  }
}

export function buildIssuesByIdsVariables(issueIds: string[]) {
  return { issueIds }
}
