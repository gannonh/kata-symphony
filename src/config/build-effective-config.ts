import { coerceInteger, coerceStringList, normalizeStateMap } from './coerce.js'
import { DEFAULTS } from './defaults.js'
import { ConfigValidationError } from './errors.js'
import { resolveEnvToken, resolvePathValue } from './resolve.js'
import type { CodexConfig, EffectiveConfig } from './types.js'

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function readSection(raw: Record<string, unknown>, key: string): Record<string, unknown> {
  const section = raw[key]
  return isRecord(section) ? section : {}
}

function coerceString(value: unknown, fallback: string): string {
  if (typeof value !== 'string') {
    return fallback
  }

  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : fallback
}

function coerceOptionalString(value: unknown): string | undefined {
  if (typeof value !== 'string') {
    return undefined
  }

  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : undefined
}

function coerceScript(value: unknown): string | null {
  if (typeof value !== 'string') {
    return null
  }

  return value.trim().length > 0 ? value : null
}

export function buildEffectiveConfig(rawConfig: Record<string, unknown>, env: NodeJS.ProcessEnv): EffectiveConfig {
  const raw = isRecord(rawConfig) ? rawConfig : {}

  const trackerRaw = readSection(raw, 'tracker')
  const pollingRaw = readSection(raw, 'polling')
  const workspaceRaw = readSection(raw, 'workspace')
  const hooksRaw = readSection(raw, 'hooks')
  const agentRaw = readSection(raw, 'agent')
  const codexRaw = readSection(raw, 'codex')

  const trackerKind = coerceString(trackerRaw.kind, DEFAULTS.tracker.kind)
  if (trackerKind !== 'linear') {
    throw new ConfigValidationError('unsupported_tracker_kind', 'tracker.kind', `Unsupported tracker.kind: ${trackerKind}`)
  }

  const trackerApiKeyRaw = coerceString(trackerRaw.api_key, DEFAULTS.tracker.api_key)
  const trackerApiKey = resolveEnvToken(trackerApiKeyRaw, env).trim()
  if (!trackerApiKey) {
    throw new ConfigValidationError(
      'missing_tracker_api_key',
      'tracker.api_key',
      'tracker.api_key is required after $VAR resolution',
    )
  }

  const trackerProjectSlug = coerceString(trackerRaw.project_slug, DEFAULTS.tracker.project_slug)
  if (!trackerProjectSlug) {
    throw new ConfigValidationError(
      'missing_tracker_project_slug',
      'tracker.project_slug',
      'tracker.project_slug is required for tracker.kind=linear',
    )
  }

  const codexCommand = coerceString(codexRaw.command, DEFAULTS.codex.command)
  if (!codexCommand) {
    throw new ConfigValidationError('missing_codex_command', 'codex.command', 'codex.command is required')
  }

  const activeStates = coerceStringList(trackerRaw.active_states)
  const terminalStates = coerceStringList(trackerRaw.terminal_states)
  const stateLimits = normalizeStateMap(agentRaw.max_concurrent_agents_by_state)
  const approvalPolicy = coerceOptionalString(codexRaw.approval_policy)
  const threadSandbox = coerceOptionalString(codexRaw.thread_sandbox)
  const turnSandboxPolicy = coerceOptionalString(codexRaw.turn_sandbox_policy)

  const codex: CodexConfig = {
    command: codexCommand,
    turn_timeout_ms: coerceInteger(codexRaw.turn_timeout_ms, DEFAULTS.codex.turn_timeout_ms),
    read_timeout_ms: coerceInteger(codexRaw.read_timeout_ms, DEFAULTS.codex.read_timeout_ms),
    stall_timeout_ms: coerceInteger(codexRaw.stall_timeout_ms, DEFAULTS.codex.stall_timeout_ms),
  }

  if (approvalPolicy) {
    codex.approval_policy = approvalPolicy
  }

  if (threadSandbox) {
    codex.thread_sandbox = threadSandbox
  }

  if (turnSandboxPolicy) {
    codex.turn_sandbox_policy = turnSandboxPolicy
  }

  return {
    tracker: {
      kind: trackerKind,
      endpoint: coerceString(trackerRaw.endpoint, DEFAULTS.tracker.endpoint),
      api_key: trackerApiKey,
      project_slug: trackerProjectSlug,
      active_states: activeStates.length > 0 ? activeStates : [...DEFAULTS.tracker.active_states],
      terminal_states: terminalStates.length > 0 ? terminalStates : [...DEFAULTS.tracker.terminal_states],
    },
    polling: {
      interval_ms: coerceInteger(pollingRaw.interval_ms, DEFAULTS.polling.interval_ms),
    },
    workspace: {
      root: resolvePathValue(coerceString(workspaceRaw.root, DEFAULTS.workspace.root), env),
    },
    hooks: {
      after_create: coerceScript(hooksRaw.after_create),
      before_run: coerceScript(hooksRaw.before_run),
      after_run: coerceScript(hooksRaw.after_run),
      before_remove: coerceScript(hooksRaw.before_remove),
      timeout_ms: coerceInteger(hooksRaw.timeout_ms, DEFAULTS.hooks.timeout_ms),
    },
    agent: {
      max_concurrent_agents: coerceInteger(agentRaw.max_concurrent_agents, DEFAULTS.agent.max_concurrent_agents),
      max_turns: coerceInteger(agentRaw.max_turns, DEFAULTS.agent.max_turns),
      max_retry_backoff_ms: coerceInteger(agentRaw.max_retry_backoff_ms, DEFAULTS.agent.max_retry_backoff_ms),
      max_concurrent_agents_by_state: Object.keys(stateLimits).length > 0 ? stateLimits : { ...DEFAULTS.agent.max_concurrent_agents_by_state },
    },
    codex,
  }
}
