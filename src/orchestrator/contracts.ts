import type { ConfigProvider } from '../config/contracts.js'
import type {
  AgentRunner,
  WorkerAttemptRunner,
  WorkspaceManager,
} from '../execution/contracts.js'
import type { Logger } from '../observability/contracts.js'
import type { TrackerClient } from '../tracker/contracts.js'
export type {
  DispatchPreflightError,
  DispatchPreflightErrorCode,
  DispatchPreflightResult,
} from './preflight/index.js'
export { isDispatchPreflightFailure } from './preflight/index.js'
export type {
  OrchestratorClaimState,
  OrchestratorState,
  RunningEntry,
} from './runtime/index.js'
export { deriveClaimState } from './runtime/index.js'

export interface Orchestrator {
  start(): Promise<void>
  stop(): Promise<void>
}

export interface OrchestratorDeps {
  config: ConfigProvider
  tracker: TrackerClient
  workspace: WorkspaceManager
  agentRunner: AgentRunner
  workerAttemptRunner: WorkerAttemptRunner
  logger: Logger
}

export function createNoopOrchestrator(deps: OrchestratorDeps): Orchestrator {
  void deps
  return {
    async start() {},
    async stop() {},
  }
}
