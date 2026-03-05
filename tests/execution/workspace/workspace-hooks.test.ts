import { describe, expect, it, vi } from 'vitest'

import { runWorkspaceHook } from '../../../src/execution/workspace/hooks.js'

describe('workspace hook runner', () => {
  it('throws fatal timeout for before_run', async () => {
    const runCommand = vi.fn().mockResolvedValue({
      timed_out: true,
      exit_code: null,
      stderr: 'timeout',
    })

    await expect(
      runWorkspaceHook({
        hook: 'before_run',
        script: 'echo hi',
        timeout_ms: 1000,
        cwd: '/tmp/ws/KAT-227',
        runCommand,
      }),
    ).rejects.toMatchObject({ code: 'workspace_hook_timeout' })
  })

  it('returns ignored failure for after_run', async () => {
    const runCommand = vi.fn().mockResolvedValue({
      timed_out: false,
      exit_code: 42,
      stderr: 'bad',
    })

    const result = await runWorkspaceHook({
      hook: 'after_run',
      script: 'exit 42',
      timeout_ms: 1000,
      cwd: '/tmp/ws/KAT-227',
      runCommand,
    })

    expect(result.outcome).toBe('ignored_failure')
  })

  it('throws fatal hook failure for non-timeout before_run exits', async () => {
    const runCommand = vi.fn().mockResolvedValue({
      timed_out: false,
      exit_code: 9,
      stderr: 'bad exit',
    })

    await expect(
      runWorkspaceHook({
        hook: 'before_run',
        script: 'exit 9',
        timeout_ms: 1000,
        cwd: '/tmp/ws/KAT-227',
        runCommand,
      }),
    ).rejects.toMatchObject({ code: 'workspace_hook_failed' })
  })

  it('returns ignored_failure for nonfatal timeout', async () => {
    const runCommand = vi.fn().mockResolvedValue({
      timed_out: true,
      exit_code: null,
      stderr: 'timeout',
    })

    const result = await runWorkspaceHook({
      hook: 'before_remove',
      script: 'sleep 1',
      timeout_ms: 10,
      cwd: '/tmp/ws/KAT-227',
      runCommand,
    })

    expect(result.outcome).toBe('ignored_failure')
  })

  it('returns ok for successful hook execution', async () => {
    const runCommand = vi.fn().mockResolvedValue({
      timed_out: false,
      exit_code: 0,
      stderr: '',
    })

    const result = await runWorkspaceHook({
      hook: 'before_run',
      script: 'echo ok',
      timeout_ms: 1000,
      cwd: '/tmp/ws/KAT-227',
      runCommand,
    })

    expect(result.outcome).toBe('ok')
  })
})
