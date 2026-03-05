import type { EffectiveConfig } from './types.js'

interface LegacyRuntimeConfig {
  poll_interval_ms: number
  max_concurrent_agents: number
}

export type ConfigSnapshot = EffectiveConfig & Partial<LegacyRuntimeConfig> & Record<string, unknown>

export type ConfigInput = Partial<ConfigSnapshot> & Record<string, unknown>

export interface ConfigProvider {
  getSnapshot(): ConfigSnapshot
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
  const initialSnapshot: ConfigSnapshot = structuredClone({
    ...createDefaultEffectiveConfig(),
    ...snapshot,
  })

  return {
    getSnapshot: () => structuredClone(initialSnapshot),
  }
}
