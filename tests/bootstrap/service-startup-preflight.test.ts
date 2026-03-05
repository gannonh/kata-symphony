import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
import type { DispatchPreflightError } from '../../src/orchestrator/preflight/index.js'
import { createService, startService } from '../../src/bootstrap/service.js'

describe('startService startup preflight gate', () => {
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

  it('rejects with dispatch_preflight_failed when startup preflight fails', async () => {
    const service = createService()
    const orchestratorStartSpy = vi.fn(async () => {})
    const loggerErrorSpy = vi.fn()

    service.orchestrator.start = orchestratorStartSpy
    service.logger.error = loggerErrorSpy

    const errors: DispatchPreflightError[] = [
      {
        code: 'tracker_api_key_missing',
        message: 'tracker.api_key is required after resolution',
        source: 'config',
        field: 'tracker.api_key',
      },
    ]

    await expect(
      startService(service, {
        runStartupPreflight: async () => ({ ok: false, errors }),
      }),
    ).rejects.toMatchObject({
      code: 'dispatch_preflight_failed',
    })

    expect(orchestratorStartSpy).not.toHaveBeenCalled()
    expect(loggerErrorSpy).toHaveBeenCalledTimes(1)
  })
})
