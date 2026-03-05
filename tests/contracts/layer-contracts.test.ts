import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'

import { createStaticConfigProvider } from '../../src/config/contracts.js'
import type { TrackerClient } from '../../src/tracker/contracts.js'
import type { WorkspaceManager, AgentRunner } from '../../src/execution/contracts.js'
import type { Logger } from '../../src/observability/contracts.js'
import { createNoopOrchestrator } from '../../src/orchestrator/contracts.js'

describe('layer contract surface', () => {
  it('exports required layer contracts', () => {
    expect(typeof createStaticConfigProvider).toBe('function')
    expect(typeof createNoopOrchestrator).toBe('function')

    const tracker: TrackerClient | null = null
    const workspace: WorkspaceManager | null = null
    const agent: AgentRunner | null = null
    const logger: Logger | null = null

    expect(tracker).toBeNull()
    expect(workspace).toBeNull()
    expect(agent).toBeNull()
    expect(logger).toBeNull()
  })

  it('keeps domain model free of layer imports', () => {
    const source = readFileSync('src/domain/models.ts', 'utf8')
    expect(source).not.toMatch(/from '..\/config/)
    expect(source).not.toMatch(/from '..\/tracker/)
    expect(source).not.toMatch(/from '..\/orchestrator/)
    expect(source).not.toMatch(/from '..\/execution/)
    expect(source).not.toMatch(/from '..\/observability/)
  })
})
