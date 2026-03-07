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
export type WorkerAttemptNormalReasonCode =
  | 'stopped_non_active_state'
  | 'stopped_max_turns_reached'
export type WorkerAttemptAbnormalReasonCode = Exclude<
  WorkerAttemptReasonCode,
  WorkerAttemptNormalReasonCode
>

interface WorkerAttemptOutcomeBase {
  turns_executed: number
  final_issue_state: string | null
}

export interface WorkerAttemptNormalOutcome extends WorkerAttemptOutcomeBase {
  kind: 'normal'
  reason_code: WorkerAttemptNormalReasonCode
}

export interface WorkerAttemptAbnormalOutcome extends WorkerAttemptOutcomeBase {
  kind: 'abnormal'
  reason_code: WorkerAttemptAbnormalReasonCode
}

export type WorkerAttemptOutcome =
  | WorkerAttemptNormalOutcome
  | WorkerAttemptAbnormalOutcome

export interface WorkerAttemptResult {
  attempt: RunAttempt
  session: LiveSession | null
  outcome: WorkerAttemptOutcome
}

export interface WorkerAttemptCodexEvent {
  issue_id: string
  issue_identifier: string
  event: 'turn_completed'
  turn_number: number
  timestamp: string
  session: LiveSession | null
}

export interface WorkerAttemptRunOptions {
  onCodexEvent?: (event: WorkerAttemptCodexEvent) => void
}

export interface WorkerAttemptRunner {
  run(
    issue: Issue,
    attempt: number | null,
    options?: WorkerAttemptRunOptions,
  ): Promise<WorkerAttemptResult>
}
