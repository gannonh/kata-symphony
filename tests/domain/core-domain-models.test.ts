import { describe, expect, it } from 'vitest'
import type {
  Issue,
  WorkflowDefinition,
  Workspace,
  RunAttempt,
  LiveSession,
  RetryEntry,
  OrchestratorRuntimeState,
} from '../../src/domain/models.js'
import { DOMAIN_MODELS_SCHEMA_VERSION } from '../../src/domain/models.js'

const issueFixture: Issue = {
  id: 'issue-1',
  identifier: 'KAT-221',
  title: 'Bootstrap service skeleton',
  description: 'spec mapping',
  priority: 1,
  state: 'Todo',
  branch_name: 'feature/kat-221',
  url: 'https://linear.app/kata-sh/issue/KAT-221',
  labels: ['area:symphony'],
  blocked_by: [{ id: 'issue-0', identifier: 'KAT-255', state: 'Done' }],
  created_at: '2026-03-05T00:00:00Z',
  updated_at: '2026-03-05T00:00:00Z',
}

describe('core domain model contracts', () => {
  it('exposes a runtime schema version marker', () => {
    expect(DOMAIN_MODELS_SCHEMA_VERSION).toBe(1)
  })

  it('supports Section 4 Issue + WorkflowDefinition shape', () => {
    const workflow: WorkflowDefinition = {
      config: { polling: { interval_ms: 30000 } },
      prompt_template: 'Issue: {{ issue.identifier }}',
    }

    expect(issueFixture.identifier).toBe('KAT-221')
    expect(workflow.prompt_template).toContain('{{ issue.identifier }}')
  })

  it('supports Section 4 runtime entities', () => {
    const workspace: Workspace = {
      path: '/tmp/symphony/KAT-221',
      workspace_key: 'KAT-221',
      created_now: true,
    }

    const attempt: RunAttempt = {
      issue_id: 'issue-1',
      issue_identifier: 'KAT-221',
      attempt: null,
      workspace_path: workspace.path,
      started_at: '2026-03-05T00:00:00Z',
      status: 'running',
    }

    const session: LiveSession = {
      session_id: 'thread-1-turn-1',
      thread_id: 'thread-1',
      turn_id: 'turn-1',
      codex_app_server_pid: null,
      last_codex_event: null,
      last_codex_timestamp: null,
      last_codex_message: null,
      codex_input_tokens: 0,
      codex_output_tokens: 0,
      codex_total_tokens: 0,
      last_reported_input_tokens: 0,
      last_reported_output_tokens: 0,
      last_reported_total_tokens: 0,
      turn_count: 0,
    }

    const retry: RetryEntry = {
      issue_id: 'issue-1',
      identifier: 'KAT-221',
      attempt: 1,
      due_at_ms: 0,
      timer_handle: null,
      error: null,
    }

    const runtime: OrchestratorRuntimeState = {
      poll_interval_ms: 30000,
      max_concurrent_agents: 5,
      running: new Map(),
      claimed: new Set(),
      retry_attempts: new Map(),
      completed: new Set(),
      codex_totals: { input_tokens: 0, output_tokens: 0, total_tokens: 0, seconds_running: 0 },
      codex_rate_limits: null,
    }

    expect(attempt.issue_identifier).toBe('KAT-221')
    expect(session.turn_count).toBe(0)
    expect(retry.attempt).toBe(1)
    expect(runtime.max_concurrent_agents).toBe(5)
  })
})
