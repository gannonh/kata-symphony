import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { createAgentRunner } from '../../../src/execution/agent-runner/index.js'

const issue = {
  id: '1',
  identifier: 'KAT-228',
  title: 'Build runner',
  description: null,
  priority: 1,
  state: 'In Progress',
  branch_name: null,
  url: null,
  labels: [],
  blocked_by: [],
  created_at: null,
  updated_at: null,
}

describe('agent runner', () => {
  it('runs startup handshake + turn and returns session metadata', async () => {
    const fixture = path.resolve('tests/fixtures/fake-codex-app-server.mjs')

    const runner = createAgentRunner({
      codex: {
        command: `node ${fixture} success`,
        turn_timeout_ms: 5000,
        read_timeout_ms: 500,
        stall_timeout_ms: 1000,
      },
      workspacePath: '/tmp',
      buildPrompt: async () => ({ ok: true as const, prompt: 'hello' }),
    })

    const result = await runner.runAttempt(issue, null)

    expect(result.attempt.status).toBe('succeeded')
    expect(result.session?.thread_id).toBe('thread-1')
    expect(result.session?.turn_id).toBe('turn-1')
    expect(result.session?.codex_total_tokens).toBe(12)
  })
})
