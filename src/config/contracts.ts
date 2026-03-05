import type {
  AgentConfig,
  CodexConfig,
  EffectiveConfig,
  HooksConfig,
  PollingConfig,
  TrackerConfig,
  WorkspaceConfig,
} from './types.js'
import type { WorkflowDefinition } from '../domain/models.js'

interface LegacyRuntimeConfig {
  poll_interval_ms: number
  max_concurrent_agents: number
}

export type ConfigSnapshot = EffectiveConfig & Partial<LegacyRuntimeConfig>

export interface ConfigInput extends Partial<LegacyRuntimeConfig> {
  tracker?: Partial<TrackerConfig>
  polling?: Partial<PollingConfig>
  workspace?: Partial<WorkspaceConfig>
  hooks?: Partial<HooksConfig>
  agent?: Partial<AgentConfig>
  codex?: Partial<CodexConfig>
}

export interface ConfigProvider {
  getSnapshot(): ConfigSnapshot
}

export type ReloadResult = { applied: true } | { applied: false; error: unknown }

export interface ReloadableConfigProvider extends ConfigProvider {
  reload(nextWorkflow: WorkflowDefinition): Promise<ReloadResult>
}

function createDefaultEffectiveConfig(): EffectiveConfig {
  return {
    tracker: {
      kind: '',
      endpoint: '',
      api_key: '',
      project_slug: '',
      active_states: [],
      terminal_states: [],
    },
    polling: {
      interval_ms: 0,
    },
    workspace: {
      root: '',
    },
    hooks: {
      after_create: null,
      before_run: null,
      after_run: null,
      before_remove: null,
      timeout_ms: 0,
    },
    agent: {
      max_concurrent_agents: 0,
      max_turns: 0,
      max_retry_backoff_ms: 0,
      max_concurrent_agents_by_state: {},
    },
    codex: {
      command: '',
      turn_timeout_ms: 0,
      read_timeout_ms: 0,
      stall_timeout_ms: 0,
    },
  }
}

export function createStaticConfigProvider(snapshot: ConfigInput): ConfigProvider {
  const defaults = createDefaultEffectiveConfig()
  const merged: ConfigSnapshot = {
    tracker: { ...defaults.tracker, ...(snapshot.tracker ?? {}) },
    polling: { ...defaults.polling, ...(snapshot.polling ?? {}) },
    workspace: { ...defaults.workspace, ...(snapshot.workspace ?? {}) },
    hooks: { ...defaults.hooks, ...(snapshot.hooks ?? {}) },
    agent: { ...defaults.agent, ...(snapshot.agent ?? {}) },
    codex: { ...defaults.codex, ...(snapshot.codex ?? {}) },
  }

  if (snapshot.poll_interval_ms !== undefined) {
    merged.poll_interval_ms = snapshot.poll_interval_ms
  }

  if (snapshot.max_concurrent_agents !== undefined) {
    merged.max_concurrent_agents = snapshot.max_concurrent_agents
  }

  const initialSnapshot: ConfigSnapshot = structuredClone(merged)

  return {
    getSnapshot: () => structuredClone(initialSnapshot),
  }
}
