import type {
  CodexTotals,
  Issue,
  OrchestratorRuntimeState,
  RetryEntry,
} from '../../domain/models.js'
import type { WorkerAttemptResult } from '../../execution/contracts.js'

export interface RunningEntry {
  issue: Issue
  identifier: string
  workerPromise: Promise<WorkerAttemptResult> | null
  retry_attempt: number | null
  started_at: string
  session_id: string | null
  codex_app_server_pid: string | null
  last_codex_event: string | null
  last_codex_timestamp: string | null
  last_codex_message: string | null
  codex_input_tokens: number
  codex_output_tokens: number
  codex_total_tokens: number
  last_reported_input_tokens: number
  last_reported_output_tokens: number
  last_reported_total_tokens: number
}

export type OrchestratorClaimState =
  | 'unclaimed'
  | 'claimed_running'
  | 'claimed_retry_queued'
  | 'released'

export type OrchestratorState = Omit<OrchestratorRuntimeState, 'running'> & {
  running: Map<string, RunningEntry>
  retry_attempts: Map<string, RetryEntry>
  codex_totals: CodexTotals
}

export function deriveClaimState(
  issueId: string,
  state: OrchestratorState,
): OrchestratorClaimState {
  // Active ownership structures are authoritative for the derived view.
  // When memberships overlap inconsistently, prefer the most active runtime
  // bookkeeping over terminal history: running > retry queued > released.
  if (state.running.has(issueId)) {
    return 'claimed_running'
  }

  if (state.retry_attempts.has(issueId)) {
    return 'claimed_retry_queued'
  }

  if (state.completed.has(issueId)) {
    return 'released'
  }

  return 'unclaimed'
}
