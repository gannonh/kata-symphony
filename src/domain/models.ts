export const DOMAIN_MODELS_SCHEMA_VERSION = 1

export interface BlockerRef {
  id: string | null
  identifier: string | null
  state: string | null
}

export interface Issue {
  id: string
  identifier: string
  title: string
  description: string | null
  priority: number | null
  state: string
  branch_name: string | null
  url: string | null
  labels: string[]
  blocked_by: BlockerRef[]
  created_at: string | null
  updated_at: string | null
}

export interface WorkflowDefinition {
  config: Record<string, unknown>
  prompt_template: string
}

export interface Workspace {
  path: string
  workspace_key: string
  created_now: boolean
}

export interface RunAttempt {
  issue_id: string
  issue_identifier: string
  attempt: number | null
  workspace_path: string
  started_at: string
  status: string
  error?: string
}

export interface LiveSession {
  session_id: string
  thread_id: string
  turn_id: string
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
  turn_count: number
}

export interface RetryEntry {
  issue_id: string
  identifier: string
  attempt: number
  due_at_ms: number
  timer_handle: unknown
  error: string | null
}

export interface CodexTotals {
  input_tokens: number
  output_tokens: number
  total_tokens: number
  seconds_running: number
}

export interface OrchestratorRuntimeState {
  poll_interval_ms: number
  max_concurrent_agents: number
  running: Map<string, unknown>
  claimed: Set<string>
  retry_attempts: Map<string, RetryEntry>
  completed: Set<string>
  codex_totals: CodexTotals
  codex_rate_limits: unknown
}
