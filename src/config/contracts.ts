import type {
  AgentConfig,
  CodexConfig,
  EffectiveConfig,
  HooksConfig,
  PollingConfig,
  TrackerConfig,
  WorkspaceConfig,
} from './types.js'
import { DEFAULTS } from './defaults.js'
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
  return structuredClone(DEFAULTS)
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
    merged.agent.max_concurrent_agents = snapshot.max_concurrent_agents
  } else if (snapshot.agent?.max_concurrent_agents !== undefined) {
    merged.max_concurrent_agents = snapshot.agent.max_concurrent_agents
  }

  const initialSnapshot: ConfigSnapshot = structuredClone(merged)

  return {
    getSnapshot: () => structuredClone(initialSnapshot),
  }
}
