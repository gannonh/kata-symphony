import { describe, expect, it } from 'vitest'
import { runMain } from '../../src/bootstrap/main-entry.js'

describe('main entry bootstrap guard', () => {
  it('handles startup failure without unhandled rejection', async () => {
    const previousExitCode = process.exitCode
    process.exitCode = undefined

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

    process.exitCode = previousExitCode
  })

  it('sets exit code even when error reporter throws', async () => {
    const previousExitCode = process.exitCode
    process.exitCode = undefined

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
    process.exitCode = previousExitCode
  })
})
