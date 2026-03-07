import { normalizeIssueState } from '../../domain/normalization.js'
import type { Issue } from '../../domain/models.js'
import type { OrchestratorState } from './contracts.js'

export interface DispatchSelectionOptions {
  activeStates: readonly string[]
  terminalStates: readonly string[]
  maxConcurrentAgents: number
  perStateLimits: Readonly<Record<string, number>>
}

const TODO_STATE = normalizeIssueState('Todo')
const UNKNOWN_PRIORITY_RANK = Number.POSITIVE_INFINITY
const UNKNOWN_CREATED_AT_RANK = Number.POSITIVE_INFINITY

export function sortCandidatesForDispatch<T extends Pick<Issue, 'created_at' | 'identifier' | 'priority'>>(
  issues: readonly T[],
): T[] {
  return [...issues].sort((left, right) => {
    const priorityOrder = compareNumbers(
      getPriorityRank(left.priority),
      getPriorityRank(right.priority),
    )

    if (priorityOrder !== 0) {
      return priorityOrder
    }

    const createdAtOrder = compareNumbers(
      getCreatedAtRank(left.created_at),
      getCreatedAtRank(right.created_at),
    )

    if (createdAtOrder !== 0) {
      return createdAtOrder
    }

    return left.identifier.localeCompare(right.identifier)
  })
}

export function shouldDispatch(
  issue: Partial<Issue>,
  state: OrchestratorState,
  options: DispatchSelectionOptions,
): boolean {
  if (!hasRequiredStructuralFields(issue)) {
    return false
  }

  const normalizedIssueState = normalizeIssueState(issue.state)
  const activeStates = createNormalizedStateSet(options.activeStates)
  const terminalStates = createNormalizedStateSet(options.terminalStates)
  const perStateLimits = normalizePerStateLimits(options.perStateLimits)

  if (!activeStates.has(normalizedIssueState) || terminalStates.has(normalizedIssueState)) {
    return false
  }

  if (state.running.has(issue.id) || state.claimed.has(issue.id)) {
    return false
  }

  if (getAvailableGlobalSlots(state, options) <= 0) {
    return false
  }

  if (getAvailablePerStateSlots(normalizedIssueState, state, options, perStateLimits) <= 0) {
    return false
  }

  if (normalizedIssueState === TODO_STATE && !todoBlockersAreTerminal(issue, terminalStates)) {
    return false
  }

  return true
}

function hasRequiredStructuralFields(issue: Partial<Issue>): issue is Issue {
  return (
    hasNonEmptyString(issue.id) &&
    hasNonEmptyString(issue.identifier) &&
    hasNonEmptyString(issue.title) &&
    hasNonEmptyString(issue.state)
  )
}

function hasNonEmptyString(value: unknown): value is string {
  return typeof value === 'string' && value.trim().length > 0
}

function createNormalizedStateSet(states: readonly string[]): Set<string> {
  return new Set(states.map((state) => normalizeIssueState(state)))
}

function getPriorityRank(priority: number | null): number {
  if (typeof priority !== 'number' || !Number.isFinite(priority)) {
    return UNKNOWN_PRIORITY_RANK
  }

  return priority
}

function getCreatedAtRank(createdAt: string | null): number {
  if (createdAt === null) {
    return UNKNOWN_CREATED_AT_RANK
  }

  const timestamp = Date.parse(createdAt)

  if (Number.isNaN(timestamp)) {
    return UNKNOWN_CREATED_AT_RANK
  }

  return timestamp
}

function compareNumbers(left: number, right: number): number {
  if (left < right) {
    return -1
  }

  if (left > right) {
    return 1
  }

  return 0
}

function getAvailableGlobalSlots(
  state: OrchestratorState,
  options: DispatchSelectionOptions,
): number {
  return Math.max(options.maxConcurrentAgents - state.running.size, 0)
}

function getAvailablePerStateSlots(
  normalizedIssueState: string,
  state: OrchestratorState,
  options: DispatchSelectionOptions,
  perStateLimits: Readonly<Record<string, number>>,
): number {
  const runningCount = countRunningEntriesByNormalizedState(state).get(normalizedIssueState) ?? 0
  const stateLimit = perStateLimits[normalizedIssueState] ?? options.maxConcurrentAgents

  return Math.max(stateLimit - runningCount, 0)
}

function normalizePerStateLimits(
  perStateLimits: Readonly<Record<string, number>>,
): Record<string, number> {
  const normalizedLimits: Record<string, number> = {}

  for (const [state, limit] of Object.entries(perStateLimits)) {
    normalizedLimits[normalizeIssueState(state)] = limit
  }

  return normalizedLimits
}

function countRunningEntriesByNormalizedState(
  state: OrchestratorState,
): Map<string, number> {
  const counts = new Map<string, number>()

  for (const entry of state.running.values()) {
    const normalizedState = normalizeIssueState(entry.issue.state)
    counts.set(normalizedState, (counts.get(normalizedState) ?? 0) + 1)
  }

  return counts
}

function todoBlockersAreTerminal(
  issue: Issue,
  terminalStates: ReadonlySet<string>,
): boolean {
  const blockers = Array.isArray(issue.blocked_by) ? issue.blocked_by : []

  return blockers.every((blocker) => {
    if (!hasNonEmptyString(blocker.state)) {
      return false
    }

    return terminalStates.has(normalizeIssueState(blocker.state))
  })
}
