import type { WorkflowDefinition } from '../domain/models.js'
import { buildEffectiveConfig } from './build-effective-config.js'
import type { ConfigSnapshot, ReloadableConfigProvider } from './contracts.js'

export function createReloadableConfigProvider(
  initialWorkflow: WorkflowDefinition,
  env: NodeJS.ProcessEnv,
): ReloadableConfigProvider {
  let current: ConfigSnapshot = buildEffectiveConfig(initialWorkflow.config, env)

  return {
    getSnapshot: () => structuredClone(current),
    async reload(nextWorkflow: WorkflowDefinition) {
      try {
        current = buildEffectiveConfig(nextWorkflow.config, env)
        return { applied: true }
      } catch (error) {
        return { applied: false, error }
      }
    },
  }
}
