import type { Issue } from '../../domain/models.js'
import type {
  DispatchPreflightError,
  DispatchPreflightResult,
} from '../preflight/contracts.js'
import { runTickPreflightGate } from '../preflight/index.js'
import type { DispatchSelectionOptions } from './dispatch-selection.js'
import { shouldDispatch, sortCandidatesForDispatch } from './dispatch-selection.js'
import type { OrchestratorState } from './contracts.js'

export interface RunPollTickOptions {
  state: OrchestratorState
  selection: DispatchSelectionOptions
  reconcile: (state: OrchestratorState) => Promise<OrchestratorState>
  validate: () => Promise<DispatchPreflightResult>
  logFailure: (errors: DispatchPreflightError[]) => void | Promise<void>
  fetchCandidates: () => Promise<Issue[]>
  dispatchIssue: (
    state: OrchestratorState,
    issue: Issue,
    attempt: number | null,
  ) => Promise<OrchestratorState>
}

export async function runPollTick(
  options: RunPollTickOptions,
): Promise<OrchestratorState> {
  let state = await options.reconcile(options.state)

  const gate = await runTickPreflightGate({
    reconcile: async () => {},
    validate: options.validate,
    logFailure: options.logFailure,
  })

  if (!gate.dispatchAllowed) {
    return state
  }

  let issues: Issue[]
  try {
    issues = await options.fetchCandidates()
  } catch {
    return state
  }

  for (const issue of sortCandidatesForDispatch(issues)) {
    if (state.running.size >= state.max_concurrent_agents) {
      break
    }

    if (!shouldDispatch(issue, state, options.selection)) {
      continue
    }

    state = await options.dispatchIssue(state, issue, null)
  }

  return state
}
