export interface ConfigSnapshot {
  poll_interval_ms: number
  max_concurrent_agents: number
}

export interface ConfigProvider {
  getSnapshot(): ConfigSnapshot
}

export function createStaticConfigProvider(snapshot: ConfigSnapshot): ConfigProvider {
  const initialSnapshot = { ...snapshot }

  return {
    getSnapshot: () => ({ ...initialSnapshot }),
  }
}
