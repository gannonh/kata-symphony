import { beforeEach, describe, expect, it, vi } from 'vitest'

const runMainMock = vi.fn().mockResolvedValue(undefined)

vi.mock('../../src/bootstrap/main-entry.js', () => ({
  runMain: runMainMock,
}))

describe('main module', () => {
  beforeEach(() => {
    runMainMock.mockClear()
    vi.resetModules()
  })

  it('invokes runMain on module load', async () => {
    await import('../../src/main.js')
    expect(runMainMock).toHaveBeenCalledTimes(1)
  })
})
