import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
import { createService, startService, StartupPreflightError } from '../../src/bootstrap/service.js'

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

    vi.spyOn(service.config, 'getSnapshot').mockReturnValue({
      tracker: {
        kind: 'linear',
        api_key: '',
        project_slug: 'bootstrap',
      },
      codex: {
        command: 'codex app-server',
      },
    } as ReturnType<typeof service.config.getSnapshot>)

    const startupAttempt = startService(service)

    await expect(startupAttempt).rejects.toBeInstanceOf(
      StartupPreflightError,
    )

    await expect(startupAttempt).rejects.toMatchObject({
      code: 'dispatch_preflight_failed',
      message: 'startup preflight failed',
      errors: [
        expect.objectContaining({
          code: 'tracker_api_key_missing',
          source: 'config',
          field: 'tracker.api_key',
        }),
      ],
    })

    expect(orchestratorStartSpy).not.toHaveBeenCalled()
    expect(loggerErrorSpy).toHaveBeenCalledTimes(1)
    expect(loggerErrorSpy).toHaveBeenCalledWith(
      'Dispatch preflight validation failed',
      expect.objectContaining({
        phase: 'startup',
        error_codes: ['tracker_api_key_missing'],
        errors: [
          expect.objectContaining({
            code: 'tracker_api_key_missing',
            source: 'config',
            field: 'tracker.api_key',
            message: 'tracker.api_key is required after resolution',
          }),
        ],
      }),
    )
  })

  it('rejects with dispatch_preflight_failed when startup preflight logging throws', async () => {
    const service = createService()
    const orchestratorStartSpy = vi.fn(async () => {})
    const loggerErrorSpy = vi.fn(() => {
      throw new Error('logger unavailable')
    })

    service.orchestrator.start = orchestratorStartSpy
    service.logger.error = loggerErrorSpy

    vi.spyOn(service.config, 'getSnapshot').mockReturnValue({
      tracker: {
        kind: 'linear',
        api_key: '',
        project_slug: 'bootstrap',
      },
      codex: {
        command: 'codex app-server',
      },
    } as ReturnType<typeof service.config.getSnapshot>)

    await expect(startService(service)).rejects.toMatchObject({
      code: 'dispatch_preflight_failed',
      message: 'startup preflight failed',
      errors: [
        expect.objectContaining({
          code: 'tracker_api_key_missing',
          source: 'config',
          field: 'tracker.api_key',
        }),
      ],
    })

    expect(orchestratorStartSpy).not.toHaveBeenCalled()
    expect(loggerErrorSpy).toHaveBeenCalledTimes(1)
  })
})
