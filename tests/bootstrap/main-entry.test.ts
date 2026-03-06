import { describe, expect, it, vi } from 'vitest'
import { StartupPreflightError } from '../../src/bootstrap/service.js'
import { runMain } from '../../src/bootstrap/main-entry.js'

describe('main entry bootstrap guard', () => {
  it('returns cleanly on successful startup', async () => {
    const previousExitCode = process.exitCode
    process.exitCode = undefined
    try {
      await expect(runMain(async () => {})).resolves.toBeUndefined()
      expect(process.exitCode).toBeUndefined()
    } finally {
      process.exitCode = previousExitCode
    }
  })

  it('handles startup failure without unhandled rejection', async () => {
    const previousExitCode = process.exitCode
    process.exitCode = undefined
    try {
      const startupError = new Error('startup failed')
      const errors: Array<{ message: string; error: unknown }> = []

      await runMain(
        async () => {
          throw startupError
        },
        (message, error) => {
          errors.push({ message, error })
        },
      )

      expect(process.exitCode).toBe(1)
      expect(errors).toHaveLength(1)
      const firstError = errors[0]
      expect(firstError).toBeDefined()
      expect(firstError!.message).toContain('Symphony startup failed')
      expect(firstError!.error).toBe(startupError)
    } finally {
      process.exitCode = previousExitCode
    }
  })

  it('handles StartupPreflightError startup failure with exit semantics', async () => {
    const previousExitCode = process.exitCode
    process.exitCode = undefined
    try {
      const startupFailure = new StartupPreflightError([
        {
          code: 'tracker_api_key_missing',
          source: 'config',
          field: 'tracker.api_key',
          message: 'tracker.api_key is required after resolution',
        },
      ])
      const onError = vi.fn<(message: string, error: unknown) => void>()

      await runMain(
        async () => {
          throw startupFailure
        },
        onError,
      )

      expect(process.exitCode).toBe(1)
      expect(onError).toHaveBeenCalledTimes(1)
      expect(onError).toHaveBeenCalledWith(
        'Symphony startup failed',
        startupFailure,
      )
    } finally {
      process.exitCode = previousExitCode
    }
  })

  it('sets exit code even when error reporter throws', async () => {
    const previousExitCode = process.exitCode
    process.exitCode = undefined
    try {
      await expect(
        runMain(
          async () => {
            throw new Error('startup failure')
          },
          () => {
            throw new Error('error logger failure')
          },
        ),
      ).resolves.toBeUndefined()

      expect(process.exitCode).toBe(1)
    } finally {
      process.exitCode = previousExitCode
    }
  })

  it('uses default error reporter when none is provided', async () => {
    const previousExitCode = process.exitCode
    process.exitCode = undefined
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    try {
      await runMain(async () => {
        throw new Error('startup failure')
      })

      expect(errorSpy).toHaveBeenCalledWith(
        'Symphony startup failed',
        expect.any(Error),
      )
      expect(process.exitCode).toBe(1)
    } finally {
      errorSpy.mockRestore()
      process.exitCode = previousExitCode
    }
  })
})
