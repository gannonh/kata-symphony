import type { Issue } from '../../domain/models.js'
import { createLinearUnknownPayloadError } from '../errors.js'
import type { LinearIssueNode } from './types.js'

function isRequiredIssueField(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0
}

function parseIsoTimestamp(value: unknown): string | null {
  if (typeof value !== 'string') {
    return null
  }

  const parsed = Date.parse(value)
  if (Number.isNaN(parsed)) {
    return null
  }

  return new Date(parsed).toISOString()
}

function normalizePriority(value: unknown): number | null {
  return typeof value === 'number' && Number.isInteger(value) ? value : null
}

export function normalizeLinearIssue(node: LinearIssueNode): Issue {
  if (
    !isRequiredIssueField(node.id) ||
    !isRequiredIssueField(node.identifier) ||
    !isRequiredIssueField(node.title) ||
    !isRequiredIssueField(node.state?.name)
  ) {
    throw createLinearUnknownPayloadError('issue node missing required fields')
  }

  const labels = (node.labels?.nodes ?? [])
    .map((label) => (typeof label?.name === 'string' ? label.name.toLowerCase() : null))
    .filter((label): label is string => label !== null)

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
    priority: normalizePriority(node.priority),
    state: node.state.name,
    branch_name: typeof node.branchName === 'string' ? node.branchName : null,
    url: typeof node.url === 'string' ? node.url : null,
    labels,
    blocked_by: blockedBy,
    created_at: parseIsoTimestamp(node.createdAt),
    updated_at: parseIsoTimestamp(node.updatedAt),
  }
}
