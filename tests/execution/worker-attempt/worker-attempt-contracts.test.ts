import { describe, expect, it } from 'vitest'

import type { WorkerAttemptRunner } from '../../../src/execution/contracts.js'
import {
  WORKER_ATTEMPT_OUTCOME_KINDS,
  WORKER_ATTEMPT_REASON_CODES,
  type WorkerAttemptOutcome,
} from '../../../src/execution/worker-attempt/contracts.js'

describe('worker attempt contracts', () => {
  it('exposes stable outcome kinds and reason codes', () => {
    expect(WORKER_ATTEMPT_OUTCOME_KINDS).toEqual(['normal', 'abnormal'])
    expect(WORKER_ATTEMPT_REASON_CODES).toEqual([
      'stopped_non_active_state',
      'stopped_max_turns_reached',
      'workspace_error',
      'before_run_hook_error',
      'agent_session_startup_error',
      'prompt_error',
      'agent_turn_error',
      'issue_state_refresh_error',
    ])
  })

  it('adds a worker-attempt runner contract to the execution layer', () => {
    const runner: WorkerAttemptRunner = {
      async run() {
        throw new Error('not implemented')
      },
    }

    expect(typeof runner.run).toBe('function')
  })

  it('discriminates reason codes by outcome kind at the type level', () => {
    const normalOutcome = {
      kind: 'normal',
      reason_code: 'stopped_max_turns_reached',
      turns_executed: 3,
      final_issue_state: 'In Progress',
    } satisfies WorkerAttemptOutcome

    const abnormalOutcome = {
      kind: 'abnormal',
      reason_code: 'workspace_error',
      turns_executed: 0,
      final_issue_state: null,
    } satisfies WorkerAttemptOutcome

    // @ts-expect-error abnormal reason codes must not be accepted for normal outcomes
    const impossibleOutcome: WorkerAttemptOutcome = {
      kind: 'normal',
      reason_code: 'workspace_error',
      turns_executed: 0,
      final_issue_state: null,
    }

    expect(normalOutcome.reason_code).toBe('stopped_max_turns_reached')
    expect(abnormalOutcome.reason_code).toBe('workspace_error')
    expect(impossibleOutcome).toBeDefined()
  })
})
