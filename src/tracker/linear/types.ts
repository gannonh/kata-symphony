export interface LinearGraphQLError {
  message: string
  [key: string]: unknown
}

export interface LinearStateNode {
  name?: string | null
}

export interface LinearLabelNode {
  name?: string | null
}

export interface LinearIssueRelationIssueNode {
  id?: string | null
  identifier?: string | null
  state?: LinearStateNode | null
}

export interface LinearIssueRelationNode {
  type?: string | null
  issue?: LinearIssueRelationIssueNode | null
}

export interface LinearIssueNode {
  id?: string | null
  identifier?: string | null
  title?: string | null
  description?: string | null
  priority?: number | null
  state?: LinearStateNode | null
  branchName?: string | null
  url?: string | null
  labels?: { nodes?: LinearLabelNode[] | null } | null
  issueRelations?: { nodes?: LinearIssueRelationNode[] | null } | null
  createdAt?: string | null
  updatedAt?: string | null
}
