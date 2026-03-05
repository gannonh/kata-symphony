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
    tracker: {},
    polling: {},
    workspace: {},
    hooks: {},
    agent: {},
    codex: {},
  }
}

export function createStaticConfigProvider(snapshot: ConfigInput): ConfigProvider {
  const initialSnapshot: ConfigSnapshot = { ...createDefaultEffectiveConfig(), ...snapshot }

  return {
    getSnapshot: () => ({ ...initialSnapshot }),
  }
}
