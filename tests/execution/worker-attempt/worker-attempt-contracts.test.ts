import { describe, expect, it } from 'vitest'

import type { WorkerAttemptRunner } from '../../../src/execution/contracts.js'
import {
  WORKER_ATTEMPT_OUTCOME_KINDS,
  WORKER_ATTEMPT_REASON_CODES,
} from '../../../src/execution/worker-attempt/contracts.js'

describe('worker attempt contracts', () => {
  it('exposes stable outcome kinds and reason codes', () => {
    expect(WORKER_ATTEMPT_OUTCOME_KINDS).toEqual(['normal', 'abnormal'])
    expect(WORKER_ATTEMPT_REASON_CODES).toEqual(
      expect.arrayContaining([
        'stopped_non_active_state',
        'stopped_max_turns_reached',
        'workspace_error',
        'before_run_hook_error',
        'agent_session_startup_error',
        'prompt_error',
        'agent_turn_error',
        'issue_state_refresh_error',
      ]),
    )
  })

  it('adds a worker-attempt runner contract to the execution layer', () => {
    const runner: WorkerAttemptRunner = {
      async run() {
        throw new Error('not implemented')
      },
    }

    expect(typeof runner.run).toBe('function')
  })
})
