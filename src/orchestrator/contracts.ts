import type { ConfigProvider } from '../config/contracts.js'
import type { AgentRunner, WorkspaceManager } from '../execution/contracts.js'
import type { Logger } from '../observability/contracts.js'
import type { TrackerClient } from '../tracker/contracts.js'

export interface Orchestrator {
  start(): Promise<void>
  stop(): Promise<void>
}

export interface OrchestratorDeps {
  config: ConfigProvider
  tracker: TrackerClient
  workspace: WorkspaceManager
  agentRunner: AgentRunner
  logger: Logger
}

export function createNoopOrchestrator(deps: OrchestratorDeps): Orchestrator {
  void deps
  return {
    async start() {},
    async stop() {},
  }
}
