const WORKSPACE_KEY_INVALID = /[^A-Za-z0-9._-]/g

export function sanitizeWorkspaceKey(identifier: string): string {
  const sanitized = identifier.replace(WORKSPACE_KEY_INVALID, '_')

  if (sanitized.length === 0) {
    return '_'
  }

  if (sanitized === '.' || sanitized === '..') {
    return sanitized.replace(/\./g, '_')
  }

  return sanitized
}

export function normalizeIssueState(state: string): string {
  return state.trim().toLowerCase()
}

export function makeSessionId(threadId: string, turnId: string): string {
  return `${threadId}-${turnId}`
}
