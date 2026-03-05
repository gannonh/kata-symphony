import os from 'node:os'
import path from 'node:path'
import type { EffectiveConfig } from './types.js'

export const DEFAULTS: EffectiveConfig = {
  tracker: {
    kind: 'linear',
    endpoint: 'https://api.linear.app/graphql',
    api_key: '',
    project_slug: '',
    active_states: ['Todo', 'In Progress'],
    terminal_states: ['Closed', 'Cancelled', 'Canceled', 'Duplicate', 'Done'],
  },
  polling: {
    interval_ms: 30000,
  },
  workspace: {
    root: path.join(os.tmpdir(), 'symphony_workspaces'),
  },
  hooks: {
    after_create: null,
    before_run: null,
    after_run: null,
    before_remove: null,
    timeout_ms: 60000,
  },
  agent: {
    max_concurrent_agents: 10,
    max_turns: 20,
    max_retry_backoff_ms: 300000,
    max_concurrent_agents_by_state: {},
  },
  codex: {
    command: 'codex app-server',
    turn_timeout_ms: 3600000,
    read_timeout_ms: 5000,
    stall_timeout_ms: 300000,
  },
}
