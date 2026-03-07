import { loadWorkflowDefinition } from '../workflow/index.js'
import type { Orchestrator, OrchestratorDeps } from './contracts.js'
import {
  applyCodexUpdate,
  claimRunningIssue,
  createInitialOrchestratorState,
  deriveWorkerExitIntent,
  recordCompletion,
  releaseIssue,
  runPollTick,
  type DispatchSelectionOptions,
  type OrchestratorState,
} from './runtime/index.js'
import {
  logPreflightFailure,
  validateDispatchPreflight,
} from './preflight/index.js'

type StateUpdater =
  | OrchestratorState
  | ((state: OrchestratorState) => OrchestratorState)

export function createOrchestrator(deps: OrchestratorDeps): Orchestrator {
  let state = applySnapshot(
    createInitialOrchestratorState(deps.config.getSnapshot()),
    deps,
  )
  let stopped = true
  let timer: ReturnType<typeof setTimeout> | null = null
  let mutationQueue = Promise.resolve()

  const enqueueStateUpdate = (updater: StateUpdater): Promise<void> => {
    const applyUpdate = async () => {
      state =
        typeof updater === 'function'
          ? applySnapshot(updater(state), deps)
          : applySnapshot(updater, deps)
    }

    const next = mutationQueue.then(applyUpdate, applyUpdate)
    mutationQueue = next.then(() => undefined, () => undefined)
    return next
  }

  const scheduleNextTick = () => {
    if (stopped) {
      return
    }

    timer = setTimeout(() => {
      void tick()
    }, state.poll_interval_ms)
    timer.unref?.()
  }

  const tick = async () => {
    if (stopped) {
      return
    }

    let releaseDispatchCallbacks = () => {}
    const dispatchCallbacksReady = new Promise<void>((resolve) => {
      releaseDispatchCallbacks = resolve
    })

    try {
      const nextState = await runPollTick({
        state,
        selection: getDispatchSelection(deps),
        reconcile: async (currentState) => currentState,
        validate: () =>
          validateDispatchPreflight({
            loadWorkflow: loadWorkflowDefinition,
            getSnapshot: () => deps.config.getSnapshot(),
          }),
        logFailure: (errors) =>
          logPreflightFailure(deps.logger, 'tick', errors),
        fetchCandidates: () => deps.tracker.fetchCandidates(),
        dispatchIssue: async (currentState, issue, attempt) => {
          const workerPromise = deps.workerAttemptRunner.run(issue, attempt, {
            onCodexEvent: (event) => {
              void dispatchCallbacksReady.then(() =>
                enqueueStateUpdate((latestState) =>
                  applyCodexUpdate(latestState, event.issue_id, {
                    session: event.session,
                  }),
                ),
              )
            },
          })

          void workerPromise
            .then((result) =>
              dispatchCallbacksReady.then(() =>
                enqueueStateUpdate((latestState) => {
                  const withCompletion =
                    result.outcome.kind === 'normal'
                      ? recordCompletion(latestState, issue.id)
                      : latestState
                  const intent = deriveWorkerExitIntent(
                    withCompletion,
                    issue.id,
                    result,
                  )

                  deps.logger.info('orchestrator_worker_exit', {
                    issue_id: issue.id,
                    issue_identifier: issue.identifier,
                    intent_kind: intent.kind,
                    retry_attempt:
                      intent.kind === 'retry' ? intent.attempt : null,
                    retry_kind:
                      intent.kind === 'retry' ? intent.retry_kind : null,
                    error: intent.kind === 'retry' ? intent.error : null,
                  })

                  return releaseIssue(withCompletion, issue.id)
                }),
              ),
            )
            .catch((error: unknown) =>
              dispatchCallbacksReady.then(() =>
                enqueueStateUpdate((latestState) => {
                  deps.logger.error('orchestrator_worker_exit', {
                    issue_id: issue.id,
                    issue_identifier: issue.identifier,
                    intent_kind: 'release',
                    error:
                      error instanceof Error ? error.message : String(error),
                  })

                  return releaseIssue(latestState, issue.id)
                }),
              ),
            )

          return claimRunningIssue(currentState, issue, {
            workerPromise,
            retry_attempt: attempt,
            started_at: new Date().toISOString(),
          })
        },
      })

      state = applySnapshot(nextState, deps)
    } catch (error) {
      deps.logger.error('orchestrator_tick_failed', {
        error: error instanceof Error ? error.message : String(error),
      })
    } finally {
      releaseDispatchCallbacks()
      scheduleNextTick()
    }
  }

  return {
    async start() {
      if (!stopped) {
        return
      }

      stopped = false
      await tick()
    },

    async stop() {
      stopped = true

      if (timer) {
        clearTimeout(timer)
        timer = null
      }

      await mutationQueue
    },
  }
}

function applySnapshot(
  state: OrchestratorState,
  deps: OrchestratorDeps,
): OrchestratorState {
  const snapshot = deps.config.getSnapshot()

  return {
    ...state,
    poll_interval_ms: snapshot.polling.interval_ms,
    max_concurrent_agents: snapshot.agent.max_concurrent_agents,
  }
}

function getDispatchSelection(
  deps: OrchestratorDeps,
): DispatchSelectionOptions {
  const snapshot = deps.config.getSnapshot()

  return {
    activeStates: snapshot.tracker.active_states,
    terminalStates: snapshot.tracker.terminal_states,
    perStateLimits: snapshot.agent.max_concurrent_agents_by_state,
  }
}
