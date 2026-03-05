import { describe, expect, it } from 'vitest'
import { buildEffectiveConfig } from '../../src/config/build-effective-config.js'
import { ConfigValidationError } from '../../src/config/errors.js'

describe('effective config builder', () => {
  it('maps workflow front matter to typed config with defaults', () => {
    const config = buildEffectiveConfig(
      {
        tracker: {
          kind: 'linear',
          project_slug: 'proj',
          api_key: '$LINEAR_API_KEY',
          active_states: ['Todo', 'In Progress'],
          terminal_states: 'Done, Cancelled',
        },
        workspace: { root: '~/ws' },
        hooks: {
          after_create: 'echo hi',
          before_run: '',
          timeout_ms: '61000',
        },
        agent: {
          max_concurrent_agents: '11',
          max_concurrent_agents_by_state: { ' In Progress ': '2', Done: 0 },
        },
        codex: {
          approval_policy: 'on-request',
          thread_sandbox: 'workspace-write',
          turn_sandbox_policy: 'restricted',
          turn_timeout_ms: '7200000',
        },
      },
      { LINEAR_API_KEY: 'token', HOME: '/Users/test' },
    )

    expect(config.tracker.endpoint).toBe('https://api.linear.app/graphql')
    expect(config.tracker.terminal_states).toEqual(['Done', 'Cancelled'])
    expect(config.polling.interval_ms).toBe(30000)
    expect(config.workspace.root).toBe('/Users/test/ws')
    expect(config.hooks.after_create).toBe('echo hi')
    expect(config.hooks.before_run).toBeNull()
    expect(config.hooks.timeout_ms).toBe(61000)
    expect(config.agent.max_concurrent_agents).toBe(11)
    expect(config.agent.max_concurrent_agents_by_state).toEqual({ 'in progress': 2 })
    expect(config.codex.approval_policy).toBe('on-request')
    expect(config.codex.thread_sandbox).toBe('workspace-write')
    expect(config.codex.turn_sandbox_policy).toBe('restricted')
    expect(config.codex.turn_timeout_ms).toBe(7200000)
  })

  it('throws typed error for missing required values', () => {
    expect(() => buildEffectiveConfig({ tracker: { kind: 'linear' } }, {})).toThrow(ConfigValidationError)
    try {
      buildEffectiveConfig({ tracker: { kind: 'linear' } }, {})
    } catch (error) {
      const err = error as ConfigValidationError
      expect(err.code).toBe('missing_tracker_api_key')
      expect(err.path).toBe('tracker.api_key')
    }
  })

  it('throws for unsupported tracker kind', () => {
    expect(() =>
      buildEffectiveConfig(
        { tracker: { kind: 'jira', api_key: '$LINEAR_API_KEY', project_slug: 'proj' } },
        { LINEAR_API_KEY: 'token' },
      ),
    ).toThrow(ConfigValidationError)
    try {
      buildEffectiveConfig(
        { tracker: { kind: 'jira', api_key: '$LINEAR_API_KEY', project_slug: 'proj' } },
        { LINEAR_API_KEY: 'token' },
      )
    } catch (error) {
      const err = error as ConfigValidationError
      expect(err.code).toBe('unsupported_tracker_kind')
      expect(err.path).toBe('tracker.kind')
    }
  })

  it('throws for missing tracker.project_slug when required', () => {
    expect(() =>
      buildEffectiveConfig({ tracker: { kind: 'linear', api_key: '$LINEAR_API_KEY', project_slug: '  ' } }, { LINEAR_API_KEY: 'token' }),
    ).toThrow(ConfigValidationError)
    try {
      buildEffectiveConfig({ tracker: { kind: 'linear', api_key: '$LINEAR_API_KEY', project_slug: '  ' } }, { LINEAR_API_KEY: 'token' })
    } catch (error) {
      const err = error as ConfigValidationError
      expect(err.code).toBe('missing_tracker_project_slug')
      expect(err.path).toBe('tracker.project_slug')
    }
  })

  it('throws for explicitly empty codex.command', () => {
    expect(() =>
      buildEffectiveConfig(
        {
          tracker: { kind: 'linear', project_slug: 'proj', api_key: '$LINEAR_API_KEY' },
          codex: { command: '   ' },
        },
        { LINEAR_API_KEY: 'token' },
      ),
    ).toThrow(ConfigValidationError)
    try {
      buildEffectiveConfig(
        {
          tracker: { kind: 'linear', project_slug: 'proj', api_key: '$LINEAR_API_KEY' },
          codex: { command: '   ' },
        },
        { LINEAR_API_KEY: 'token' },
      )
    } catch (error) {
      const err = error as ConfigValidationError
      expect(err.code).toBe('missing_codex_command')
      expect(err.path).toBe('codex.command')
    }
  })

  it('throws for workspace.root that resolves to empty string', () => {
    expect(() =>
      buildEffectiveConfig(
        {
          tracker: { kind: 'linear', project_slug: 'proj', api_key: '$LINEAR_API_KEY' },
          workspace: { root: '$MISSING_ENV' },
        },
        { LINEAR_API_KEY: 'token' },
      ),
    ).toThrow(ConfigValidationError)
    try {
      buildEffectiveConfig(
        {
          tracker: { kind: 'linear', project_slug: 'proj', api_key: '$LINEAR_API_KEY' },
          workspace: { root: '$MISSING_ENV' },
        },
        { LINEAR_API_KEY: 'token' },
      )
    } catch (error) {
      const err = error as ConfigValidationError
      expect(err.code).toBe('missing_workspace_root')
      expect(err.path).toBe('workspace.root')
    }
  })

  it('drops blank optional codex string fields', () => {
    const config = buildEffectiveConfig(
      {
        tracker: { kind: 'linear', project_slug: 'proj', api_key: '$LINEAR_API_KEY' },
        codex: {
          approval_policy: '   ',
          thread_sandbox: '   ',
          turn_sandbox_policy: '   ',
        },
      },
      { LINEAR_API_KEY: 'token' },
    )

    expect(config.codex.approval_policy).toBeUndefined()
    expect(config.codex.thread_sandbox).toBeUndefined()
    expect(config.codex.turn_sandbox_policy).toBeUndefined()
  })

  it('handles non-object raw config input defensively', () => {
    expect(() => buildEffectiveConfig([] as unknown as Record<string, unknown>, {})).toThrow(/tracker\.api_key/)
  })
})
