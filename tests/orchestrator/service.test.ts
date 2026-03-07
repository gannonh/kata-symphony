import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Issue, LiveSession } from '../../src/domain/models.js'
import type { WorkerAttemptResult } from '../../src/execution/contracts.js'
import type { OrchestratorDeps, OrchestratorState } from '../../src/orchestrator/contracts.js'
import { releaseIssue } from '../../src/orchestrator/runtime/index.js'

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

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T | PromiseLike<T>) => void
}

function createDeferred<T>(): Deferred<T> {
  let resolve: Deferred<T>['resolve'] = () => {}
  const promise = new Promise<T>((innerResolve) => {
    resolve = innerResolve
  })

  return {
    promise,
    resolve,
  }
}

describe('createOrchestrator', () => {
  interface MockRunPollTickOptions {
    state: OrchestratorState
    dispatchIssue: (
      state: OrchestratorState,
      issue: Issue,
      attempt: number | null,
    ) => Promise<OrchestratorState>
    logFailure: (errors: Array<{
      code: string
      source: 'workflow' | 'config'
      field?: string
      message: string
    }>) => Promise<void> | void
  }

  function createIssue(overrides: Partial<Issue> = {}): Issue {
    return {
      id: 'issue-1',
      identifier: 'KAT-230',
      title: 'Run orchestrator tick',
      description: null,
      priority: 1,
      state: 'Todo',
      branch_name: null,
      url: null,
      labels: [],
      blocked_by: [],
      created_at: '2026-03-07T00:00:00Z',
      updated_at: '2026-03-07T00:00:00Z',
      ...overrides,
    }
  }

  async function createDeps(
    overrides: Partial<OrchestratorDeps> = {},
  ): Promise<OrchestratorDeps> {
    const { createStaticConfigProvider } = await import(
      '../../src/config/contracts.js'
    )

    return {
      config: createStaticConfigProvider({
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
      }),
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
      ...overrides,
    }
  }

  beforeEach(() => {
    vi.useFakeTimers()
    harness.runPollTick.mockClear()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('runs immediately on start, reschedules on the poll interval, and stops cleanly', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const orchestrator = createOrchestrator(await createDeps())

    await orchestrator.start()
    expect(harness.runPollTick).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(30_000)
    expect(harness.runPollTick).toHaveBeenCalledTimes(2)

    await orchestrator.stop()
    await vi.advanceTimersByTimeAsync(30_000)
    expect(harness.runPollTick).toHaveBeenCalledTimes(2)
  })

  it('does nothing when start is called while already running', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')

    let resolveTick: () => void = () => {
      throw new Error('tick resolver was not initialized')
    }
    harness.runPollTick.mockImplementationOnce(
      ({ state }) =>
        new Promise<OrchestratorState>((resolve) => {
          resolveTick = () => resolve(state as OrchestratorState)
        }),
    )

    const orchestrator = createOrchestrator(await createDeps())
    const firstStart = orchestrator.start()
    await Promise.resolve()

    await expect(orchestrator.start()).resolves.toBeUndefined()
    expect(harness.runPollTick).toHaveBeenCalledTimes(1)

    resolveTick()
    await firstStart
    await orchestrator.stop()
  })

  it('logs tick failures and continues scheduling', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const logger = { info: vi.fn(), error: vi.fn() }

    harness.runPollTick
      .mockRejectedValueOnce(new Error('boom'))
      .mockImplementation(async ({ state }) => state)

    const orchestrator = createOrchestrator(await createDeps({ logger }))

    await orchestrator.start()
    expect(logger.error).toHaveBeenCalledWith('orchestrator_tick_failed', {
      error: 'boom',
    })

    await vi.advanceTimersByTimeAsync(30_000)
    expect(harness.runPollTick).toHaveBeenCalledTimes(2)

    await orchestrator.stop()
  })

  it('stringifies non-Error tick failures before logging them', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const logger = { info: vi.fn(), error: vi.fn() }

    harness.runPollTick
      .mockRejectedValueOnce('boom')
      .mockImplementation(async ({ state }) => state)

    const orchestrator = createOrchestrator(await createDeps({ logger }))

    await orchestrator.start()
    expect(logger.error).toHaveBeenCalledWith('orchestrator_tick_failed', {
      error: 'boom',
    })

    await vi.advanceTimersByTimeAsync(30_000)
    expect(harness.runPollTick).toHaveBeenCalledTimes(2)

    await orchestrator.stop()
  })

  it('releases timers without scheduling another tick when stopped during an in-flight tick', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')

    let resolveTick: () => void = () => {
      throw new Error('tick resolver was not initialized')
    }
    harness.runPollTick.mockImplementationOnce(
      ({ state }) =>
        new Promise<OrchestratorState>((resolve) => {
          resolveTick = () => resolve(state as OrchestratorState)
        }),
    )

    const setTimeoutSpy = vi.spyOn(globalThis, 'setTimeout')
    const orchestrator = createOrchestrator(await createDeps())
    const startPromise = orchestrator.start()
    await Promise.resolve()

    let stopSettled = false
    const stopPromise = orchestrator.stop().then(() => {
      stopSettled = true
    })
    await Promise.resolve()
    expect(stopSettled).toBe(false)

    resolveTick()
    await startPromise
    await stopPromise

    expect(setTimeoutSpy).not.toHaveBeenCalled()
    setTimeoutSpy.mockRestore()
  })

  it('waits for the active tick to finish and skips dispatch once stop is requested', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const issue = createIssue()
    const tickDeferred = createDeferred<void>()
    const workerAttemptRunner = {
      run: vi.fn(async () => {
        throw new Error('worker should not start after stop')
      }),
    }

    harness.runPollTick.mockImplementationOnce(async (options) => {
      const typedOptions = options as MockRunPollTickOptions
      await tickDeferred.promise
      return typedOptions.dispatchIssue(typedOptions.state, issue, null)
    })

    const orchestrator = createOrchestrator(
      await createDeps({
        workerAttemptRunner,
      }),
    )
    const startPromise = orchestrator.start()
    await Promise.resolve()

    let stopSettled = false
    const stopPromise = orchestrator.stop().then(() => {
      stopSettled = true
    })
    await Promise.resolve()
    expect(stopSettled).toBe(false)

    tickDeferred.resolve()
    await startPromise
    await stopPromise

    expect(workerAttemptRunner.run).not.toHaveBeenCalled()
  })

  it('returns immediately when a queued timer fires after stop', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')

    let scheduledTick: (() => void) | undefined
    const setTimeoutSpy = vi
      .spyOn(globalThis, 'setTimeout')
      .mockImplementation(((
        handler: TimerHandler,
      ) => {
        scheduledTick = handler as () => void
        return 1 as unknown as ReturnType<typeof setTimeout>
      }) as unknown as typeof setTimeout)
    const clearTimeoutSpy = vi.spyOn(globalThis, 'clearTimeout')
    const orchestrator = createOrchestrator(await createDeps())

    await orchestrator.start()
    expect(harness.runPollTick).toHaveBeenCalledTimes(1)

    await orchestrator.stop()
    scheduledTick?.()
    await Promise.resolve()

    expect(harness.runPollTick).toHaveBeenCalledTimes(1)
    expect(clearTimeoutSpy).toHaveBeenCalled()

    clearTimeoutSpy.mockRestore()
    setTimeoutSpy.mockRestore()
  })

  it('routes tick preflight failures through the orchestrator logger callback', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const logger = { info: vi.fn(), error: vi.fn() }

    harness.runPollTick.mockImplementation(async (options) => {
      const typedOptions = options as MockRunPollTickOptions

      await typedOptions.logFailure([
        {
          code: 'workflow_invalid',
          source: 'workflow',
          field: 'workflow',
          message: 'bad workflow',
        },
      ])

      return typedOptions.state
    })

    const orchestrator = createOrchestrator(await createDeps({ logger }))
    await orchestrator.start()

    expect(logger.error).toHaveBeenCalledWith(
      'Dispatch preflight validation failed',
      expect.objectContaining({
        phase: 'tick',
        error_codes: ['workflow_invalid'],
      }),
    )

    await orchestrator.stop()
  })

  it('routes codex events and normal worker exits back through orchestrator state', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const logger = { info: vi.fn(), error: vi.fn() }
    const issue = createIssue()
    const session: LiveSession = {
      session_id: 'thread-1-turn-1',
      thread_id: 'thread-1',
      turn_id: 'turn-1',
      codex_app_server_pid: '123',
      last_codex_event: 'turn/completed',
      last_codex_timestamp: '2026-03-07T00:00:01Z',
      last_codex_message: 'completed',
      codex_input_tokens: 4,
      codex_output_tokens: 5,
      codex_total_tokens: 9,
      last_reported_input_tokens: 4,
      last_reported_output_tokens: 5,
      last_reported_total_tokens: 9,
      turn_count: 1,
    }
    const workerResult: WorkerAttemptResult = {
      attempt: {
        issue_id: issue.id,
        issue_identifier: issue.identifier,
        attempt: null,
        workspace_path: '/tmp/symphony/KAT-230',
        started_at: '2026-03-07T00:00:00Z',
        status: 'succeeded',
      },
      session,
      outcome: {
        kind: 'normal',
        reason_code: 'stopped_max_turns_reached',
        turns_executed: 1,
        final_issue_state: 'Todo',
      },
    }

    harness.runPollTick.mockImplementation(async (options) => {
      const typedOptions = options as MockRunPollTickOptions

      if (
        typedOptions.state.running.size === 0 &&
        typedOptions.state.completed.size === 0
      ) {
        return typedOptions.dispatchIssue(typedOptions.state, issue, null)
      }

      return typedOptions.state
    })

    const orchestrator = createOrchestrator(
      await createDeps({
        logger,
        workerAttemptRunner: {
          async run(_issue, _attempt, runOptions) {
            runOptions?.onCodexEvent?.({
              issue_id: issue.id,
              issue_identifier: issue.identifier,
              event: 'turn_completed',
              turn_number: 1,
              timestamp: '2026-03-07T00:00:01Z',
              session,
            })
            return workerResult
          },
        },
      }),
    )

    await orchestrator.start()
    await vi.advanceTimersByTimeAsync(30_000)

    const secondTickState = harness.runPollTick.mock.calls[1]?.[0]
      ?.state as OrchestratorState
    expect(secondTickState.codex_totals.total_tokens).toBe(9)
    expect(secondTickState.completed.has(issue.id)).toBe(true)
    expect(secondTickState.running.has(issue.id)).toBe(false)
    expect(secondTickState.claimed.has(issue.id)).toBe(false)
    expect(logger.info).toHaveBeenCalledWith(
      'orchestrator_worker_exit',
      expect.objectContaining({
        issue_id: issue.id,
        intent_kind: 'retry',
        retry_kind: 'continuation',
      }),
    )
    expect(logger.error).not.toHaveBeenCalled()

    await orchestrator.stop()
  })

  it('logs and releases worker failures routed from dispatch callbacks', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const logger = { info: vi.fn(), error: vi.fn() }
    const issue = createIssue()

    harness.runPollTick.mockImplementation(async (options) => {
      const typedOptions = options as MockRunPollTickOptions

      if (
        typedOptions.state.running.size === 0 &&
        typedOptions.state.claimed.size === 0
      ) {
        return typedOptions.dispatchIssue(typedOptions.state, issue, null)
      }

      return typedOptions.state
    })

    const orchestrator = createOrchestrator(
      await createDeps({
        logger,
        workerAttemptRunner: {
          async run() {
            return Promise.reject('worker exploded')
          },
        },
      }),
    )

    await orchestrator.start()
    await vi.advanceTimersByTimeAsync(30_000)

    const secondTickState = harness.runPollTick.mock.calls[1]?.[0]
      ?.state as OrchestratorState
    expect(secondTickState.running.size).toBe(0)
    expect(secondTickState.claimed.size).toBe(0)
    expect(secondTickState.completed.size).toBe(0)
    expect(logger.error).toHaveBeenCalledWith(
      'orchestrator_worker_exit',
      expect.objectContaining({
        issue_id: issue.id,
        intent_kind: 'release',
        error: 'worker exploded',
      }),
    )

    await orchestrator.stop()
  })

  it('waits for in-flight worker lifecycle callbacks before stop returns', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const issue = createIssue()
    const workerDeferred = createDeferred<WorkerAttemptResult>()

    harness.runPollTick.mockImplementation(async (options) => {
      const typedOptions = options as MockRunPollTickOptions

      if (
        typedOptions.state.running.size === 0 &&
        typedOptions.state.claimed.size === 0
      ) {
        return typedOptions.dispatchIssue(typedOptions.state, issue, null)
      }

      return typedOptions.state
    })

    const orchestrator = createOrchestrator(
      await createDeps({
        workerAttemptRunner: {
          async run() {
            return workerDeferred.promise
          },
        },
      }),
    )

    await orchestrator.start()

    let stopSettled = false
    const stopPromise = orchestrator.stop().then(() => {
      stopSettled = true
    })
    await Promise.resolve()
    await Promise.resolve()
    expect(stopSettled).toBe(false)

    workerDeferred.resolve({
      attempt: {
        issue_id: issue.id,
        issue_identifier: issue.identifier,
        attempt: null,
        workspace_path: '/tmp/symphony/KAT-230',
        started_at: '2026-03-07T00:00:00Z',
        status: 'succeeded',
      },
      session: null,
      outcome: {
        kind: 'normal',
        reason_code: 'stopped_max_turns_reached',
        turns_executed: 1,
        final_issue_state: 'Todo',
      },
    })
    await stopPromise

    expect(stopSettled).toBe(true)
  })

  it('logs and swallows worker lifecycle state update failures', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const issue = createIssue()
    const logger = {
      info: vi.fn(() => {
        throw new Error('log exploded')
      }),
      error: vi.fn(),
    }

    harness.runPollTick.mockImplementation(async (options) => {
      const typedOptions = options as MockRunPollTickOptions

      if (
        typedOptions.state.running.size === 0 &&
        typedOptions.state.claimed.size === 0
      ) {
        return typedOptions.dispatchIssue(typedOptions.state, issue, null)
      }

      return typedOptions.state
    })

    const orchestrator = createOrchestrator(
      await createDeps({
        logger,
        workerAttemptRunner: {
          async run() {
            return {
              attempt: {
                issue_id: issue.id,
                issue_identifier: issue.identifier,
                attempt: null,
                workspace_path: '/tmp/symphony/KAT-230',
                started_at: '2026-03-07T00:00:00Z',
                status: 'succeeded',
              },
              session: null,
              outcome: {
                kind: 'normal' as const,
                reason_code: 'stopped_max_turns_reached' as const,
                turns_executed: 1,
                final_issue_state: 'Todo',
              },
            }
          },
        },
      }),
    )

    await orchestrator.start()
    await vi.advanceTimersByTimeAsync(30_000)

    expect(logger.error).toHaveBeenCalledWith(
      'orchestrator_state_update_failed',
      expect.objectContaining({
        issue_id: issue.id,
        error: 'log exploded',
      }),
    )

    await orchestrator.stop()
  })

  it('logs and swallows codex event state update failures', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const issue = createIssue()
    const logger = { info: vi.fn(), error: vi.fn() }
    const { createStaticConfigProvider } = await import(
      '../../src/config/contracts.js'
    )

    let snapshotReads = 0
    const config = {
      getSnapshot: vi.fn(() => {
        snapshotReads += 1
        if (snapshotReads === 5) {
          throw new Error('snapshot exploded')
        }

        return createStaticConfigProvider({
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
        }).getSnapshot()
      }),
    }

    harness.runPollTick.mockImplementation(async (options) => {
      const typedOptions = options as MockRunPollTickOptions

      if (
        typedOptions.state.running.size === 0 &&
        typedOptions.state.claimed.size === 0
      ) {
        return typedOptions.dispatchIssue(typedOptions.state, issue, null)
      }

      return typedOptions.state
    })

    const orchestrator = createOrchestrator(
      await createDeps({
        config,
        logger,
        workerAttemptRunner: {
          async run(_issue, _attempt, runOptions) {
            runOptions?.onCodexEvent?.({
              issue_id: issue.id,
              issue_identifier: issue.identifier,
              event: 'turn_completed',
              turn_number: 1,
              timestamp: '2026-03-07T00:00:01Z',
              session: null,
            })

            return {
              attempt: {
                issue_id: issue.id,
                issue_identifier: issue.identifier,
                attempt: null,
                workspace_path: '/tmp/symphony/KAT-230',
                started_at: '2026-03-07T00:00:00Z',
                status: 'succeeded',
              },
              session: null,
              outcome: {
                kind: 'normal' as const,
                reason_code: 'stopped_non_active_state' as const,
                turns_executed: 1,
                final_issue_state: 'Done',
              },
            }
          },
        },
      }),
    )

    await orchestrator.start()
    await vi.advanceTimersByTimeAsync(30_000)

    expect(logger.error).toHaveBeenCalledWith(
      'orchestrator_state_update_failed',
      expect.objectContaining({
        issue_id: issue.id,
        error: 'snapshot exploded',
      }),
    )

    await orchestrator.stop()
  })

  it('swallows secondary logger failures while reporting state update failures', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const issue = createIssue()
    const logger = {
      info: vi.fn(() => {
        throw new Error('log exploded')
      }),
      error: vi.fn(() => {
        throw new Error('secondary logger exploded')
      }),
    }

    harness.runPollTick.mockImplementation(async (options) => {
      const typedOptions = options as MockRunPollTickOptions

      if (
        typedOptions.state.running.size === 0 &&
        typedOptions.state.claimed.size === 0
      ) {
        return typedOptions.dispatchIssue(typedOptions.state, issue, null)
      }

      return typedOptions.state
    })

    const orchestrator = createOrchestrator(
      await createDeps({
        logger,
        workerAttemptRunner: {
          async run() {
            return {
              attempt: {
                issue_id: issue.id,
                issue_identifier: issue.identifier,
                attempt: null,
                workspace_path: '/tmp/symphony/KAT-230',
                started_at: '2026-03-07T00:00:00Z',
                status: 'succeeded',
              },
              session: null,
              outcome: {
                kind: 'normal' as const,
                reason_code: 'stopped_max_turns_reached' as const,
                turns_executed: 1,
                final_issue_state: 'Todo',
              },
            }
          },
        },
      }),
    )

    await orchestrator.start()
    await vi.advanceTimersByTimeAsync(30_000)

    expect(logger.error).toHaveBeenCalledWith(
      'orchestrator_state_update_failed',
      expect.objectContaining({
        issue_id: issue.id,
        error: 'log exploded',
      }),
    )

    await expect(orchestrator.stop()).resolves.toBeUndefined()
  })

  it('clears the active tick when tick error logging throws', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const logger = {
      info: vi.fn(),
      error: vi.fn(() => {
        throw new Error('logger exploded')
      }),
    }

    harness.runPollTick.mockRejectedValueOnce(new Error('boom'))

    const orchestrator = createOrchestrator(await createDeps({ logger }))

    await expect(orchestrator.start()).rejects.toThrow('logger exploded')
    await expect(orchestrator.stop()).resolves.toBeUndefined()
  })

  it('logs Error-backed worker failures using the error message', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const logger = { info: vi.fn(), error: vi.fn() }
    const issue = createIssue()

    harness.runPollTick.mockImplementation(async (options) => {
      const typedOptions = options as MockRunPollTickOptions

      if (
        typedOptions.state.running.size === 0 &&
        typedOptions.state.claimed.size === 0
      ) {
        return typedOptions.dispatchIssue(typedOptions.state, issue, null)
      }

      return typedOptions.state
    })

    const orchestrator = createOrchestrator(
      await createDeps({
        logger,
        workerAttemptRunner: {
          async run() {
            return Promise.reject(new Error('worker exploded'))
          },
        },
      }),
    )

    await orchestrator.start()
    await vi.advanceTimersByTimeAsync(30_000)

    expect(logger.error).toHaveBeenCalledWith(
      'orchestrator_worker_exit',
      expect.objectContaining({
        issue_id: issue.id,
        intent_kind: 'release',
        error: 'worker exploded',
      }),
    )

    await orchestrator.stop()
  })

  it('logs abnormal worker exits without recording completion bookkeeping', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const logger = { info: vi.fn(), error: vi.fn() }
    const issue = createIssue()

    harness.runPollTick.mockImplementation(async (options) => {
      const typedOptions = options as MockRunPollTickOptions

      if (
        typedOptions.state.running.size === 0 &&
        typedOptions.state.claimed.size === 0
      ) {
        return typedOptions.dispatchIssue(typedOptions.state, issue, 2)
      }

      return typedOptions.state
    })

    const orchestrator = createOrchestrator(
      await createDeps({
        logger,
        workerAttemptRunner: {
          async run() {
            return {
              attempt: {
                issue_id: issue.id,
                issue_identifier: issue.identifier,
                attempt: 2,
                workspace_path: '/tmp/symphony/KAT-230',
                started_at: '2026-03-07T00:00:00Z',
                status: 'failed',
              },
              session: null,
              outcome: {
                kind: 'abnormal' as const,
                reason_code: 'agent_turn_error' as const,
                turns_executed: 1,
                final_issue_state: 'Todo',
              },
            }
          },
        },
      }),
    )

    await orchestrator.start()
    await vi.advanceTimersByTimeAsync(30_000)

    const secondTickState = harness.runPollTick.mock.calls[1]?.[0]
      ?.state as OrchestratorState
    expect(secondTickState.completed.size).toBe(0)
    expect(logger.info).toHaveBeenCalledWith(
      'orchestrator_worker_exit',
      expect.objectContaining({
        issue_id: issue.id,
        intent_kind: 'retry',
        retry_kind: 'failure',
        retry_attempt: 3,
        error: 'worker exited: agent_turn_error',
      }),
    )

    await orchestrator.stop()
  })

  it('logs released worker intents with null retry fields when the issue was already released', async () => {
    const { createOrchestrator } = await import('../../src/orchestrator/service.js')
    const logger = { info: vi.fn(), error: vi.fn() }
    const issue = createIssue()

    let resolveWorker: ((result: WorkerAttemptResult) => void) | undefined
    harness.runPollTick.mockImplementation(async (options) => {
      const typedOptions = options as MockRunPollTickOptions

      if (
        typedOptions.state.running.size === 0 &&
        typedOptions.state.claimed.size === 0 &&
        typedOptions.state.completed.size === 0
      ) {
        return typedOptions.dispatchIssue(typedOptions.state, issue, null)
      }

      if (typedOptions.state.running.has(issue.id)) {
        return releaseIssue(typedOptions.state, issue.id)
      }

      return typedOptions.state
    })

    const orchestrator = createOrchestrator(
      await createDeps({
        logger,
        workerAttemptRunner: {
          async run() {
            return new Promise<WorkerAttemptResult>((resolve) => {
              resolveWorker = resolve
            })
          },
        },
      }),
    )

    await orchestrator.start()
    await vi.advanceTimersByTimeAsync(30_000)

    resolveWorker?.({
      attempt: {
        issue_id: issue.id,
        issue_identifier: issue.identifier,
        attempt: null,
        workspace_path: '/tmp/symphony/KAT-230',
        started_at: '2026-03-07T00:00:00Z',
        status: 'succeeded',
      },
      session: null,
      outcome: {
        kind: 'normal',
        reason_code: 'stopped_max_turns_reached',
        turns_executed: 1,
        final_issue_state: 'Done',
      },
    })
    await Promise.resolve()
    await Promise.resolve()
    await vi.advanceTimersByTimeAsync(30_000)

    expect(logger.info).toHaveBeenCalledWith(
      'orchestrator_worker_exit',
      expect.objectContaining({
        issue_id: issue.id,
        intent_kind: 'release',
        retry_attempt: null,
        retry_kind: null,
        error: null,
      }),
    )

    await orchestrator.stop()
  })
})
