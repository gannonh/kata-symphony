import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { resolve, dirname } from 'node:path'

import { createStaticConfigProvider } from '../../src/config/contracts.js'
import type { TrackerClient } from '../../src/tracker/contracts.js'
import { createLinearTrackerClient } from '../../src/tracker/index.js'
import type {
  WorkspaceManager,
  AgentRunner,
  WorkerAttemptRunner,
} from '../../src/execution/contracts.js'
import type { Logger } from '../../src/observability/contracts.js'
import { createNoopOrchestrator } from '../../src/orchestrator/contracts.js'
import { loadWorkflowDefinition } from '../../src/workflow/index.js'

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
      async runBeforeRun() {},
      async runAfterRun() {},
      async removeWorkspace(id: string) {
        return { removed: true, path: `/tmp/${id}` }
      },
    }

    const agent: AgentRunner = {
      async runAttempt() { throw new Error('not implemented') },
    }

    const workerAttemptRunner: WorkerAttemptRunner = {
      async run() { throw new Error('not implemented') },
    }

    const logger: Logger = {
      info() {},
      error() {},
    }

    expect(tracker.fetchCandidates).toBeTypeOf('function')
    expect(workspace.ensureWorkspace).toBeTypeOf('function')
    expect(workspace.runBeforeRun).toBeTypeOf('function')
    expect(workspace.runAfterRun).toBeTypeOf('function')
    expect(workspace.removeWorkspace).toBeTypeOf('function')
    expect(agent.runAttempt).toBeTypeOf('function')
    expect(workerAttemptRunner.run).toBeTypeOf('function')
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

  it('loads workflow definition with expected contract shape', async () => {
    const definition = await loadWorkflowDefinition({
      cwd: '/repo',
      readFile: async () => 'Hello prompt',
    })

    expect(definition).toEqual({
      config: {},
      prompt_template: 'Hello prompt',
    })
  })

  it('builds a concrete linear tracker adapter from config layer', async () => {
    let capturedUrl: string | undefined
    let capturedBody: string | undefined
    const fetchImpl: typeof fetch = async (input, init) => {
      capturedUrl = typeof input === 'string' ? input : (input as Request).url
      capturedBody = typeof init?.body === 'string' ? init.body : undefined
      return new Response(
        JSON.stringify({
          data: {
            issues: { nodes: [], pageInfo: { hasNextPage: false, endCursor: null } },
          },
        }),
      )
    }

    const client = createLinearTrackerClient(
      createStaticConfigProvider({
        tracker: {
          kind: 'linear',
          endpoint: 'https://api.linear.app/graphql',
          api_key: 'token',
          project_slug: 'proj',
          active_states: ['Todo'],
          terminal_states: ['Done'],
        },
      }),
      fetchImpl,
    )

    await expect(client.fetchCandidates()).resolves.toEqual([])
    expect(capturedUrl).toBe('https://api.linear.app/graphql')
    expect(capturedBody).toBeDefined()
    expect(JSON.parse(capturedBody as string)).toMatchObject({
      variables: {
        projectSlug: 'proj',
        states: ['Todo'],
        first: 50,
        after: null,
      },
    })
  })
})
