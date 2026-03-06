import type { Issue, LiveSession, RunAttempt } from '../../domain/models.js'

export const WORKER_ATTEMPT_OUTCOME_KINDS = ['normal', 'abnormal'] as const
export const WORKER_ATTEMPT_REASON_CODES = [
  'stopped_non_active_state',
  'stopped_max_turns_reached',
  'workspace_error',
  'before_run_hook_error',
  'agent_session_startup_error',
  'prompt_error',
  'agent_turn_error',
  'issue_state_refresh_error',
] as const

export type WorkerAttemptOutcomeKind =
  (typeof WORKER_ATTEMPT_OUTCOME_KINDS)[number]
export type WorkerAttemptReasonCode =
  (typeof WORKER_ATTEMPT_REASON_CODES)[number]

export interface WorkerAttemptOutcome {
  kind: WorkerAttemptOutcomeKind
  reason_code: WorkerAttemptReasonCode
  turns_executed: number
  final_issue_state: string | null
}

export interface WorkerAttemptResult {
  attempt: RunAttempt
  session: LiveSession | null
  outcome: WorkerAttemptOutcome
}

export interface WorkerAttemptRunner {
  run(issue: Issue, attempt: number | null): Promise<WorkerAttemptResult>
}
