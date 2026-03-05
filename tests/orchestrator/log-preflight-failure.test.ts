import { describe, expect, it, vi } from 'vitest'
import type { Logger } from '../../src/observability/contracts.js'
import type { DispatchPreflightError } from '../../src/orchestrator/preflight/contracts.js'
import { logPreflightFailure } from '../../src/orchestrator/preflight/index.js'

describe('logPreflightFailure', () => {
  it('logs preflight error codes with phase and structured errors', () => {
    const errorSpy = vi.fn<Logger['error']>()
    const logger: Logger = {
      info: vi.fn(),
      error: errorSpy,
    }

    const errors: DispatchPreflightError[] = [
      {
        code: 'workflow_invalid',
        source: 'workflow',
        field: 'workflow',
        message: 'Workflow file cannot be loaded or parsed',
      },
      {
        code: 'tracker_api_key_missing',
        source: 'config',
        field: 'tracker.api_key',
        message: 'tracker.api_key is required after resolution',
      },
    ]

    logPreflightFailure(logger, 'startup', errors)

    expect(errorSpy).toHaveBeenCalledWith(
      'Dispatch preflight validation failed',
      {
        phase: 'startup',
        error_codes: ['workflow_invalid', 'tracker_api_key_missing'],
        errors: [
          {
            code: 'workflow_invalid',
            field: 'workflow',
            source: 'workflow',
            message: 'Workflow file cannot be loaded or parsed',
          },
          {
            code: 'tracker_api_key_missing',
            field: 'tracker.api_key',
            source: 'config',
            message: 'tracker.api_key is required after resolution',
          },
        ],
      },
    )
  })

  it('does not log sensitive context values', () => {
    const errorSpy = vi.fn<Logger['error']>()
    const logger: Logger = {
      info: vi.fn(),
      error: errorSpy,
    }

    const secret = 'lin_api_secret_123'
    logPreflightFailure(
      logger,
      'tick',
      [
        {
          code: 'tracker_api_key_missing',
          source: 'config',
          field: 'tracker.api_key',
          message: 'tracker.api_key is required after resolution',
        },
      ],
      {
        workflowPath: '/tmp/workflow.yml',
        tracker: { api_key: secret },
        tracker_api_key: secret,
      },
    )

    expect(errorSpy).toHaveBeenCalledWith(
      'Dispatch preflight validation failed',
      {
        phase: 'tick',
        error_codes: ['tracker_api_key_missing'],
        errors: [
          {
            code: 'tracker_api_key_missing',
            field: 'tracker.api_key',
            source: 'config',
            message: 'tracker.api_key is required after resolution',
          },
        ],
        workflow_path: '/tmp/workflow.yml',
      },
    )

    const [, payload] = errorSpy.mock.calls.at(-1) ?? []
    expect(JSON.stringify(payload)).not.toContain(secret)
  })
})
