export interface TrackerConfig {
  [key: string]: unknown
}

export interface PollingConfig {
  [key: string]: unknown
}

export interface WorkspaceConfig {
  [key: string]: unknown
}

export interface HooksConfig {
  [key: string]: unknown
}

export interface AgentConfig {
  [key: string]: unknown
}

export interface CodexConfig {
  [key: string]: unknown
}

export interface EffectiveConfig {
  tracker: TrackerConfig
  polling: PollingConfig
  workspace: WorkspaceConfig
  hooks: HooksConfig
  agent: AgentConfig
  codex: CodexConfig
}
