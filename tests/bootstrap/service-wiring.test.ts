import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
import { createService, startService } from '../../src/bootstrap/service.js'

describe('service bootstrap wiring', () => {
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

  it('creates dependency graph and starts without orchestration loop', async () => {
    const service = createService()
    const snapshot = service.config.getSnapshot()

    expect(snapshot.tracker.kind).toBe('linear')
    expect(snapshot.agent.max_retry_backoff_ms).toBeGreaterThan(0)
    await expect(startService(service)).resolves.toBeUndefined()
  })

  it('sanitizes workspace identifiers used for workspace pathing', async () => {
    const service = createService()
    const workspace = await service.workspace.ensureWorkspace('KAT-221/fix*scope')

    expect(workspace.workspace_key).toBe('KAT-221_fix_scope')
    expect(workspace.path).toContain('/tmp/symphony/KAT-221_fix_scope')
  })

  it('creates independent logger instances for each service bootstrap', () => {
    const first = createService()
    const second = createService()

    expect(first.logger).not.toBe(second.logger)

    const firstLoggerError = vi.fn()
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    try {
      first.logger.error = firstLoggerError
      second.logger.error('second logger message')

      expect(firstLoggerError).not.toHaveBeenCalled()
      expect(consoleErrorSpy).toHaveBeenCalledWith('second logger message', {})
    } finally {
      consoleErrorSpy.mockRestore()
    }
  })
})
