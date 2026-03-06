import { describe, expect, it, vi } from 'vitest'

import { createAgentSessionClient } from '../../../src/execution/agent-runner/session-client.js'

describe('agent session client', () => {
  it('starts one session and runs multiple turns on the same thread', async () => {
    const client = createAgentSessionClient({
      codex: {
        command: 'echo noop',
        turn_timeout_ms: 5000,
        read_timeout_ms: 500,
        stall_timeout_ms: 1000,
      },
      workspacePath: '/tmp/ws',
      spawnChild: vi.fn(),
    })

    expect(typeof client.startSession).toBe('function')
    expect(typeof client.runTurn).toBe('function')
    expect(typeof client.stopSession).toBe('function')
  })
})
