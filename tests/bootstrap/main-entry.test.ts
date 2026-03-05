import { describe, expect, it, vi } from 'vitest'
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
