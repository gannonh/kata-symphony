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
  it('returns failed attempt when prompt building fails with a string error', async () => {
    const fixture = path.resolve('tests/fixtures/fake-codex-app-server.mjs')
    const runner = createAgentRunner({
      codex: {
        command: `node ${fixture} success`,
        turn_timeout_ms: 5000,
        read_timeout_ms: 500,
        stall_timeout_ms: 1000,
      },
      workspacePath: '/tmp',
      buildPrompt: async () => ({ ok: false as const, error: 'prompt_failed' }),
    })

    const result = await runner.runAttempt(issue, 2)
    expect(result.attempt).toMatchObject({ status: 'failed', error: 'prompt_failed' })
    expect(result.attempt.attempt).toBe(2)
    expect(result.session).toBeNull()
  })

  it('returns response_error when prompt failure object has no message', async () => {
    const fixture = path.resolve('tests/fixtures/fake-codex-app-server.mjs')
    const runner = createAgentRunner({
      codex: {
        command: `node ${fixture} success`,
        turn_timeout_ms: 5000,
        read_timeout_ms: 500,
        stall_timeout_ms: 1000,
      },
      workspacePath: '/tmp',
      buildPrompt: async () => ({ ok: false as const, error: {} }),
    })

    const result = await runner.runAttempt(issue, null)
    expect(result.attempt).toMatchObject({ status: 'failed', error: 'response_error' })
    expect(result.session).toBeNull()
  })

  it('runs startup handshake + turn and returns session metadata', async () => {
    const fixture = path.resolve('tests/fixtures/fake-codex-app-server.mjs')

    const runner = createAgentRunner({
      codex: {
        command: `node ${fixture} success`,
        approval_policy: 'never',
        thread_sandbox: 'workspace-write',
        turn_sandbox_policy: 'workspace-write',
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

  it('maps read timeout to response_timeout', async () => {
    const fixture = path.resolve('tests/fixtures/fake-codex-app-server.mjs')
    const runner = createAgentRunner({
      codex: {
        command: `node ${fixture} no-initialize-response`,
        turn_timeout_ms: 5000,
        read_timeout_ms: 50,
        stall_timeout_ms: 1000,
      },
      workspacePath: '/tmp',
      buildPrompt: async () => ({ ok: true as const, prompt: 'hello' }),
    })

    const result = await runner.runAttempt(issue, null)
    expect(result.attempt.status).toBe('failed')
    expect(result.attempt).toMatchObject({ error: expect.stringContaining('response_timeout') })
    expect(result.session).toBeNull()
  })

  it('does not treat stderr diagnostic lines as protocol JSON', async () => {
    const fixture = path.resolve('tests/fixtures/fake-codex-app-server.mjs')
    const runner = createAgentRunner({
      codex: {
        command: `node ${fixture} stderr-noise`,
        turn_timeout_ms: 5000,
        read_timeout_ms: 500,
        stall_timeout_ms: 1000,
      },
      workspacePath: '/tmp',
      buildPrompt: async () => ({ ok: true as const, prompt: 'hello' }),
    })

    const result = await runner.runAttempt(issue, null)
    expect(result.attempt.status).toBe('succeeded')
  })
})
