import type { ConfigSnapshot } from '../../config/contracts.js'
import type { LoadWorkflowDefinition } from '../../workflow/contracts.js'
import type { DispatchPreflightError, DispatchPreflightResult } from './contracts.js'

interface SnapshotLookup {
  tracker?: {
    kind?: unknown
    api_key?: unknown
    project_slug?: unknown
  }
  codex?: {
    command?: unknown
  }
}

export interface ValidateDispatchPreflightOptions {
  loadWorkflow: LoadWorkflowDefinition
  getSnapshot: () => ConfigSnapshot
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function readSection(record: Record<string, unknown>, key: string): Record<string, unknown> | undefined {
  const value = record[key]
  return isRecord(value) ? value : undefined
}

function readSnapshotLookup(getSnapshot: () => ConfigSnapshot): SnapshotLookup {
  let raw: unknown
  try {
    raw = getSnapshot()
  } catch {
    return {}
  }

  if (!isRecord(raw)) {
    return {}
  }

  const tracker = readSection(raw, 'tracker')
  const codex = readSection(raw, 'codex')

  const lookup: SnapshotLookup = {}
  if (tracker) {
    lookup.tracker = {
      kind: tracker.kind,
      api_key: tracker.api_key,
      project_slug: tracker.project_slug,
    }
  }

  if (codex) {
    lookup.codex = {
      command: codex.command,
    }
  }

  return lookup
}

function readNonBlankString(value: unknown): string | null {
  if (typeof value !== 'string') {
    return null
  }

  const trimmed = value.trim()
  return trimmed.length > 0 ? trimmed : null
}

function pushConfigError(
  errors: DispatchPreflightError[],
  code: DispatchPreflightError['code'],
  message: string,
  field: string,
): void {
  errors.push({
    code,
    message,
    source: 'config',
    field,
  })
}

export async function validateDispatchPreflight(
  options: ValidateDispatchPreflightOptions,
): Promise<DispatchPreflightResult> {
  const errors: DispatchPreflightError[] = []

  try {
    await options.loadWorkflow()
  } catch {
    errors.push({
      code: 'workflow_invalid',
      message: 'Workflow file cannot be loaded or parsed',
      source: 'workflow',
      field: 'workflow',
    })
  }

  const snapshot = readSnapshotLookup(options.getSnapshot)
  const trackerKind = readNonBlankString(snapshot.tracker?.kind)
  const trackerApiKey = readNonBlankString(snapshot.tracker?.api_key)
  const trackerProjectSlug = readNonBlankString(snapshot.tracker?.project_slug)
  const codexCommand = readNonBlankString(snapshot.codex?.command)

  const selectedTrackerKind = (() => {
    if (trackerKind === null) {
      pushConfigError(
        errors,
        'tracker_kind_missing',
        'tracker.kind is required',
        'tracker.kind',
      )
      return 'linear'
    }

    if (trackerKind !== 'linear') {
      pushConfigError(
        errors,
        'tracker_kind_unsupported',
        `Unsupported tracker.kind: ${trackerKind}`,
        'tracker.kind',
      )
    }

    return trackerKind
  })()

  if (trackerApiKey === null) {
    pushConfigError(
      errors,
      'tracker_api_key_missing',
      'tracker.api_key is required after resolution',
      'tracker.api_key',
    )
  }

  if (selectedTrackerKind === 'linear' && trackerProjectSlug === null) {
    pushConfigError(
      errors,
      'tracker_project_slug_missing',
      'tracker.project_slug is required for tracker.kind=linear',
      'tracker.project_slug',
    )
  }

  if (codexCommand === null) {
    pushConfigError(
      errors,
      'codex_command_missing',
      'codex.command is required',
      'codex.command',
    )
  }

  return errors.length === 0 ? { ok: true } : { ok: false, errors }
}
