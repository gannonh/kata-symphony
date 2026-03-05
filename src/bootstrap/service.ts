import { createStaticConfigProvider } from '../config/contracts.js'
import { sanitizeWorkspaceKey } from '../domain/normalization.js'
import type { AgentRunner, WorkspaceManager } from '../execution/contracts.js'
import type { Logger } from '../observability/contracts.js'
import { createNoopOrchestrator } from '../orchestrator/contracts.js'
import type { TrackerClient } from '../tracker/contracts.js'

const logger: Logger = {
  info(message, context) {
    console.log(message, context ?? {})
  },
  error(message, context) {
    console.error(message, context ?? {})
  },
}

export interface ServiceBootstrap {
  config: ReturnType<typeof createStaticConfigProvider>
  tracker: TrackerClient
  workspace: WorkspaceManager
  agentRunner: AgentRunner
  logger: Logger
  orchestrator: ReturnType<typeof createNoopOrchestrator>
}

export function createService(): ServiceBootstrap {
  const config = createStaticConfigProvider({
    poll_interval_ms: 30000,
    max_concurrent_agents: 5,
  })

  const tracker: TrackerClient = {
    async fetchCandidates() {
      return []
    },
    async fetchIssueStatesByIds() {
      return []
    },
    async fetchTerminalIssues() {
      return []
    },
  }

  const workspace: WorkspaceManager = {
    async ensureWorkspace(issueIdentifier: string) {
      const workspaceKey = sanitizeWorkspaceKey(issueIdentifier)
      return {
        path: `/tmp/symphony/${workspaceKey}`,
        workspace_key: workspaceKey,
        created_now: false,
      }
    },
  }

  const agentRunner: AgentRunner = {
    async runAttempt() {
      throw new Error('agent runner not enabled in bootstrap mode')
    },
  }

  const orchestrator = createNoopOrchestrator({
    config,
    tracker,
    workspace,
    agentRunner,
    logger,
  })

  return { config, tracker, workspace, agentRunner, logger, orchestrator }
}

export async function startService(service = createService()): Promise<void> {
  await service.orchestrator.start()
  service.logger.info('Symphony bootstrap ok', {
    mode: 'bootstrap',
    orchestration_enabled: false,
  })
}
