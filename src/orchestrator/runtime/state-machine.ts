import type { ConfigSnapshot } from '../../config/contracts.js'
import type { Issue, LiveSession } from '../../domain/models.js'
import type { WorkerAttemptResult } from '../../execution/contracts.js'
import type { OrchestratorState, RunningEntry } from './contracts.js'

export interface ClaimRunningIssueInput {
  workerPromise: Promise<WorkerAttemptResult> | null
  retry_attempt: number | null
  started_at: string
  session?: LiveSession | null
}

export interface CodexUpdate {
  session?: Partial<LiveSession> | null
  rate_limits?: unknown
}

export interface RetryRequestIntent {
  kind: 'retry'
  issue_id: string
  identifier: string
  attempt: number
  retry_kind: 'continuation' | 'failure'
  error: string | null
}

export interface ReleaseRequestIntent {
  kind: 'release'
  issue_id: string
  identifier: string
}

export type WorkerExitIntent = RetryRequestIntent | ReleaseRequestIntent

export function createInitialOrchestratorState(
  snapshot: ConfigSnapshot,
): OrchestratorState {
  return {
    poll_interval_ms: snapshot.polling.interval_ms,
    max_concurrent_agents: snapshot.agent.max_concurrent_agents,
    running: new Map(),
    claimed: new Set(),
    retry_attempts: new Map(),
    completed: new Set(),
    codex_totals: {
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 0,
      seconds_running: 0,
    },
    codex_rate_limits: null,
  }
}

export function claimRunningIssue(
  state: OrchestratorState,
  issue: Issue,
  input: ClaimRunningIssueInput,
): OrchestratorState {
  const next = cloneState(state)

  next.running.set(issue.id, createRunningEntry(issue, input))
  next.claimed.add(issue.id)
  next.retry_attempts.delete(issue.id)

  return next
}

export function applyCodexUpdate(
  state: OrchestratorState,
  issueId: string,
  update: CodexUpdate,
): OrchestratorState {
  const next = cloneState(state)

  if (update.rate_limits !== undefined) {
    next.codex_rate_limits = update.rate_limits
  }

  const currentEntry = state.running.get(issueId)
  if (!currentEntry) {
    return next
  }

  const nextEntry = mergeSessionPatch(currentEntry, update.session ?? null)
  next.running.set(issueId, nextEntry)

  const inputDelta = Math.max(
    getReportedInputTokens(nextEntry) - getReportedInputTokens(currentEntry),
    0,
  )
  const outputDelta = Math.max(
    getReportedOutputTokens(nextEntry) - getReportedOutputTokens(currentEntry),
    0,
  )
  const totalDelta = Math.max(
    getReportedTotalTokens(nextEntry) - getReportedTotalTokens(currentEntry),
    0,
  )

  next.codex_totals = {
    ...next.codex_totals,
    input_tokens: next.codex_totals.input_tokens + inputDelta,
    output_tokens: next.codex_totals.output_tokens + outputDelta,
    total_tokens: next.codex_totals.total_tokens + totalDelta,
  }

  return next
}

export function releaseIssue(
  state: OrchestratorState,
  issueId: string,
): OrchestratorState {
  const next = cloneState(state)
  next.claimed.delete(issueId)
  next.running.delete(issueId)
  next.retry_attempts.delete(issueId)
  return next
}

export function recordCompletion(
  state: OrchestratorState,
  issueId: string,
): OrchestratorState {
  const next = cloneState(state)
  next.completed.add(issueId)
  return next
}

export function deriveWorkerExitIntent(
  state: OrchestratorState,
  issueId: string,
  result: WorkerAttemptResult,
): WorkerExitIntent {
  const runningEntry = state.running.get(issueId)
  const identifier =
    runningEntry?.identifier ?? result.attempt.issue_identifier

  if (!runningEntry) {
    return {
      kind: 'release',
      issue_id: issueId,
      identifier,
    }
  }

  if (result.outcome.kind === 'normal') {
    return {
      kind: 'retry',
      issue_id: issueId,
      identifier,
      attempt: 1,
      retry_kind: 'continuation',
      error: null,
    }
  }

  return {
    kind: 'retry',
    issue_id: issueId,
    identifier,
    attempt: (runningEntry.retry_attempt ?? 0) + 1,
    retry_kind: 'failure',
    error: `worker exited: ${result.outcome.reason_code}`,
  }
}

function cloneState(state: OrchestratorState): OrchestratorState {
  return {
    ...state,
    running: new Map(state.running),
    claimed: new Set(state.claimed),
    retry_attempts: new Map(state.retry_attempts),
    completed: new Set(state.completed),
    codex_totals: { ...state.codex_totals },
  }
}

function createRunningEntry(
  issue: Issue,
  input: ClaimRunningIssueInput,
): RunningEntry {
  return {
    issue,
    identifier: issue.identifier,
    workerPromise: input.workerPromise,
    retry_attempt: input.retry_attempt,
    started_at: input.started_at,
    session_id: input.session?.session_id ?? null,
    codex_app_server_pid: input.session?.codex_app_server_pid ?? null,
    last_codex_event: input.session?.last_codex_event ?? null,
    last_codex_timestamp: input.session?.last_codex_timestamp ?? null,
    last_codex_message: input.session?.last_codex_message ?? null,
    codex_input_tokens: input.session?.codex_input_tokens ?? 0,
    codex_output_tokens: input.session?.codex_output_tokens ?? 0,
    codex_total_tokens: input.session?.codex_total_tokens ?? 0,
    last_reported_input_tokens: input.session?.last_reported_input_tokens ?? 0,
    last_reported_output_tokens: input.session?.last_reported_output_tokens ?? 0,
    last_reported_total_tokens: input.session?.last_reported_total_tokens ?? 0,
  }
}

function mergeSessionPatch(
  entry: RunningEntry,
  session: Partial<LiveSession> | null,
): RunningEntry {
  if (!session) {
    return entry
  }

  return {
    ...entry,
    session_id: session.session_id ?? entry.session_id,
    codex_app_server_pid:
      session.codex_app_server_pid ?? entry.codex_app_server_pid,
    last_codex_event: session.last_codex_event ?? entry.last_codex_event,
    last_codex_timestamp:
      session.last_codex_timestamp ?? entry.last_codex_timestamp,
    last_codex_message:
      session.last_codex_message ?? entry.last_codex_message,
    codex_input_tokens: session.codex_input_tokens ?? entry.codex_input_tokens,
    codex_output_tokens:
      session.codex_output_tokens ?? entry.codex_output_tokens,
    codex_total_tokens: session.codex_total_tokens ?? entry.codex_total_tokens,
    last_reported_input_tokens:
      session.last_reported_input_tokens ?? entry.last_reported_input_tokens,
    last_reported_output_tokens:
      session.last_reported_output_tokens ?? entry.last_reported_output_tokens,
    last_reported_total_tokens:
      session.last_reported_total_tokens ?? entry.last_reported_total_tokens,
  }
}

function getReportedInputTokens(entry: RunningEntry): number {
  return entry.last_reported_input_tokens || entry.codex_input_tokens
}

function getReportedOutputTokens(entry: RunningEntry): number {
  return entry.last_reported_output_tokens || entry.codex_output_tokens
}

function getReportedTotalTokens(entry: RunningEntry): number {
  return entry.last_reported_total_tokens || entry.codex_total_tokens
}
