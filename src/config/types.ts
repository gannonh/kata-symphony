export interface TrackerConfig {
  kind: string
  endpoint: string
  api_key: string
  project_slug: string
  active_states: string[]
  terminal_states: string[]
}

export interface PollingConfig {
  interval_ms: number
}

export interface WorkspaceConfig {
  root: string
}

export interface HooksConfig {
  after_create: string | null
  before_run: string | null
  after_run: string | null
  before_remove: string | null
  timeout_ms: number
}

export interface AgentConfig {
  max_concurrent_agents: number
  max_turns: number
  max_retry_backoff_ms: number
  max_concurrent_agents_by_state: Record<string, number>
}

export interface CodexConfig {
  command: string
  approval_policy?: string
  thread_sandbox?: string
  turn_sandbox_policy?: string
  turn_timeout_ms: number
  read_timeout_ms: number
  stall_timeout_ms: number
}

export interface EffectiveConfig {
  tracker: TrackerConfig
  polling: PollingConfig
  workspace: WorkspaceConfig
  hooks: HooksConfig
  agent: AgentConfig
  codex: CodexConfig
}
