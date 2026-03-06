import { buildEffectiveConfig } from '../config/build-effective-config.js'
import { createStaticConfigProvider } from '../config/contracts.js'
import type { AgentRunner, WorkspaceManager } from '../execution/contracts.js'
import { createWorkspaceManager } from '../execution/workspace/index.js'
import type { Logger } from '../observability/contracts.js'
import { createNoopOrchestrator } from '../orchestrator/contracts.js'
import {
  logPreflightFailure,
  validateDispatchPreflight,
  type DispatchPreflightError,
  type DispatchPreflightResult,
} from '../orchestrator/preflight/index.js'
import type { TrackerClient } from '../tracker/contracts.js'
import { loadWorkflowDefinition } from '../workflow/index.js'

function createLogger(): Logger {
  return {
    info(message, context) {
      console.log(message, context ?? {})
    },
    error(message, context) {
      console.error(message, context ?? {})
    },
  }
}

export interface ServiceBootstrap {
  config: ReturnType<typeof createStaticConfigProvider>
  tracker: TrackerClient
  workspace: WorkspaceManager
  agentRunner: AgentRunner
  logger: Logger
  orchestrator: ReturnType<typeof createNoopOrchestrator>
}

export class StartupPreflightError extends Error {
  readonly code = 'dispatch_preflight_failed'
  readonly errors: DispatchPreflightError[]

  constructor(errors: DispatchPreflightError[]) {
    super('startup preflight failed')
    this.name = 'StartupPreflightError'
    this.errors = errors
  }
}

async function runStartupPreflight(
  service: ServiceBootstrap,
): Promise<DispatchPreflightResult> {
  return validateDispatchPreflight({
    loadWorkflow: loadWorkflowDefinition,
    getSnapshot: () => service.config.getSnapshot(),
  })
}

export function createService(): ServiceBootstrap {
  const logger = createLogger()
  const config = createStaticConfigProvider(
    buildEffectiveConfig(
      {
        tracker: {
          kind: 'linear',
          project_slug: 'bootstrap',
          api_key: '$LINEAR_API_KEY',
        },
      },
      process.env,
    ),
  )

  const tracker: TrackerClient = {
    async fetchCandidates() {
      return []
    },
    async fetchIssuesByIds() {
      return []
    },
    async fetchTerminalIssues() {
      return []
    },
  }

  const snapshot = config.getSnapshot()
  const workspace: WorkspaceManager = createWorkspaceManager({
    workspaceRoot: snapshot.workspace.root,
    hooks: snapshot.hooks,
  })

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

export async function startService(
  service = createService(),
): Promise<void> {
  const preflight = await runStartupPreflight(service)

  if (preflight.ok === false) {
    try {
      logPreflightFailure(service.logger, 'startup', preflight.errors)
    } catch (error) {
      void error
    }
    throw new StartupPreflightError(preflight.errors)
  }

  await service.orchestrator.start()
  service.logger.info('Symphony bootstrap ok', {
    mode: 'bootstrap',
    orchestration_enabled: false,
  })
}
