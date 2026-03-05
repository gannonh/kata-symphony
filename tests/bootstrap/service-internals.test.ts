import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
import { createService } from '../../src/bootstrap/service.js'

describe('bootstrap service internals', () => {
  let originalApiKey: string | undefined

  beforeAll(() => {
    originalApiKey = process.env.LINEAR_API_KEY
    process.env.LINEAR_API_KEY = process.env.LINEAR_API_KEY ?? 'test-bootstrap-key'
  })

  afterAll(() => {
    if (originalApiKey === undefined) {
      delete process.env.LINEAR_API_KEY
    } else {
      process.env.LINEAR_API_KEY = originalApiKey
    }
  })
  it('returns empty tracker results from bootstrap stubs', async () => {
    const service = createService()

    await expect(service.tracker.fetchCandidates()).resolves.toEqual([])
    await expect(service.tracker.fetchIssuesByIds(['id-1'])).resolves.toEqual(
      [],
    )
    await expect(service.tracker.fetchTerminalIssues()).resolves.toEqual([])
  })

  it('uses empty-context fallback when logger context is omitted', () => {
    const service = createService()
    const logSpy = vi.spyOn(console, 'log').mockImplementation(() => {})
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    try {
      service.logger.info('info without context')
      service.logger.error('error without context')

      expect(logSpy).toHaveBeenCalledWith('info without context', {})
      expect(errSpy).toHaveBeenCalledWith('error without context', {})
    } finally {
      logSpy.mockRestore()
      errSpy.mockRestore()
    }
  })

  it('rejects agent run attempts in bootstrap mode', async () => {
    const service = createService()

    await expect(
      service.agentRunner.runAttempt(
        {
          id: '1',
          identifier: 'KAT-1',
          title: 'title',
          description: null,
          priority: null,
          state: 'Todo',
          branch_name: null,
          url: null,
          labels: [],
          blocked_by: [],
          created_at: null,
          updated_at: null,
        },
        null,
      ),
    ).rejects.toThrow('agent runner not enabled in bootstrap mode')
  })

  it('supports noop orchestrator stop in bootstrap mode', async () => {
    const service = createService()
    await expect(service.orchestrator.stop()).resolves.toBeUndefined()
  })
})
