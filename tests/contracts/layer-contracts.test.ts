import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { resolve, dirname } from 'node:path'

import { createStaticConfigProvider } from '../../src/config/contracts.js'
import type { TrackerClient } from '../../src/tracker/contracts.js'
import type { WorkspaceManager, AgentRunner } from '../../src/execution/contracts.js'
import type { Logger } from '../../src/observability/contracts.js'
import { createNoopOrchestrator } from '../../src/orchestrator/contracts.js'

describe('layer contract surface', () => {
  it('exports required layer contracts', () => {
    expect(typeof createStaticConfigProvider).toBe('function')
    expect(typeof createNoopOrchestrator).toBe('function')

    const tracker: TrackerClient = {
      async fetchCandidates() { return [] },
      async fetchIssuesByIds() { return [] },
      async fetchTerminalIssues() { return [] },
    }

    const workspace: WorkspaceManager = {
      async ensureWorkspace(id: string) {
        return { path: `/tmp/${id}`, workspace_key: id, created_now: false }
      },
    }

    const agent: AgentRunner = {
      async runAttempt() { throw new Error('not implemented') },
    }

    const logger: Logger = {
      info() {},
      error() {},
    }

    expect(tracker.fetchCandidates).toBeTypeOf('function')
    expect(workspace.ensureWorkspace).toBeTypeOf('function')
    expect(agent.runAttempt).toBeTypeOf('function')
    expect(logger.info).toBeTypeOf('function')
  })

  it('keeps domain model free of layer imports', () => {
    const testDir = dirname(fileURLToPath(import.meta.url))
    const source = readFileSync(resolve(testDir, '../../src/domain/models.ts'), 'utf8')
    expect(source).not.toMatch(/from '..\/config/)
    expect(source).not.toMatch(/from '..\/tracker/)
    expect(source).not.toMatch(/from '..\/orchestrator/)
    expect(source).not.toMatch(/from '..\/execution/)
    expect(source).not.toMatch(/from '..\/observability/)
  })
})
