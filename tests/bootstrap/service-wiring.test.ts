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

  it('creates dependency graph and passes startup preflight before bootstrap', async () => {
    const service = createService()
    const snapshot = service.config.getSnapshot()

    expect(snapshot.tracker.kind).toBe('linear')
    expect(snapshot.tracker.api_key).toBe(process.env.LINEAR_API_KEY)
    expect(snapshot.agent.max_retry_backoff_ms).toBeGreaterThan(0)
    await expect(startService(service)).resolves.toBeUndefined()
  })

  it('blocks bootstrap when startup preflight validation fails', async () => {
    const service = createService()
    const loggerErrorSpy = vi.fn()
    const orchestratorStartSpy = vi.fn(async () => {})
    service.logger.error = loggerErrorSpy
    service.orchestrator.start = orchestratorStartSpy

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
    })
    expect(orchestratorStartSpy).not.toHaveBeenCalled()
    expect(loggerErrorSpy).toHaveBeenCalledWith(
      'Dispatch preflight validation failed',
      expect.objectContaining({
        phase: 'startup',
        error_codes: ['tracker_api_key_missing'],
      }),
    )
  })

  it('returns absolute workspace paths under configured root and sanitizes workspace identifiers', async () => {
    const service = createService()
    const snapshot = service.config.getSnapshot()
    const workspace = await service.workspace.ensureWorkspace('KAT-221/fix*scope')

    expect(workspace.path.startsWith('/')).toBe(true)
    expect(workspace.path.startsWith(snapshot.workspace.root)).toBe(true)
    expect(workspace.workspace_key).toBe('KAT-221_fix_scope')
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

  it('builds a concrete worker attempt runner in bootstrap wiring', () => {
    const service = createService()

    expect(service).toHaveProperty('workerAttemptRunner')
    expect(typeof service.workerAttemptRunner.run).toBe('function')
  })
})
