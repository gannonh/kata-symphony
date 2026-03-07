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
  | 'claimed'
  | 'running'
  | 'retry_queued'
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
  // Derive the spec-level reservation state from orchestrator-owned membership.
  // `completed` is bookkeeping only unless no live ownership or reservation
  // remains. When collections overlap, prefer the most active runtime
  // ownership: running > retry_queued > claimed > released > unclaimed.
  if (state.running.has(issueId)) {
    return 'running'
  }

  if (state.retry_attempts.has(issueId)) {
    return 'retry_queued'
  }

  if (state.claimed.has(issueId)) {
    return 'claimed'
  }

  if (state.completed.has(issueId)) {
    return 'released'
  }

  return 'unclaimed'
}
