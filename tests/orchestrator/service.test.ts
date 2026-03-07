import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const harness = vi.hoisted(() => ({
  runPollTick: vi.fn(async ({ state }: { state: unknown }) => state),
}))

vi.mock('../../src/orchestrator/runtime/index.js', async () => {
  const actual =
    await vi.importActual<typeof import('../../src/orchestrator/runtime/index.js')>(
      '../../src/orchestrator/runtime/index.js',
    )

  return {
    ...actual,
    runPollTick: harness.runPollTick,
  }
})

describe('createOrchestrator', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    harness.runPollTick.mockClear()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('runs immediately on start, reschedules on the poll interval, and stops cleanly', async () => {
    const { createStaticConfigProvider } = await import(
      '../../src/config/contracts.js'
    )
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')

    const config = createStaticConfigProvider({
      tracker: {
        kind: 'linear',
        endpoint: 'https://api.linear.app/graphql',
        api_key: 'token',
        project_slug: 'proj',
        active_states: ['Todo'],
        terminal_states: ['Done'],
      },
      polling: { interval_ms: 30_000 },
      workspace: { root: '/tmp/symphony' },
      hooks: {
        after_create: null,
        before_run: null,
        after_run: null,
        before_remove: null,
        timeout_ms: 60_000,
      },
      agent: {
        max_concurrent_agents: 2,
        max_turns: 4,
        max_retry_backoff_ms: 120_000,
        max_concurrent_agents_by_state: {},
      },
      codex: {
        command: 'codex app-server',
        turn_timeout_ms: 60_000,
        read_timeout_ms: 1_000,
        stall_timeout_ms: 10_000,
      },
    })

    const orchestrator = createOrchestrator({
      config,
      tracker: {
        async fetchCandidates() {
          return []
        },
        async fetchIssuesByIds() {
          return []
        },
        async fetchTerminalIssues() {
          return []
        },
      },
      workspace: {
        async ensureWorkspace() {
          throw new Error('not used')
        },
        async runBeforeRun() {},
        async runAfterRun() {},
        async removeWorkspace() {
          return { removed: false, path: '/tmp/symphony' }
        },
      },
      agentRunner: {
        async runAttempt() {
          throw new Error('not used')
        },
      },
      workerAttemptRunner: {
        async run() {
          throw new Error('not used')
        },
      },
      logger: {
        info: vi.fn(),
        error: vi.fn(),
      },
    })

    await orchestrator.start()
    expect(harness.runPollTick).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(30_000)
    expect(harness.runPollTick).toHaveBeenCalledTimes(2)

    await orchestrator.stop()
    await vi.advanceTimersByTimeAsync(30_000)
    expect(harness.runPollTick).toHaveBeenCalledTimes(2)
  })
})
