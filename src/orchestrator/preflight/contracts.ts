export type DispatchPreflightErrorCode =
  | 'workflow_invalid'
  | 'tracker_kind_missing'
  | 'tracker_kind_unsupported'
  | 'tracker_api_key_missing'
  | 'tracker_project_slug_missing'
  | 'codex_command_missing'

export interface DispatchPreflightError {
  code: DispatchPreflightErrorCode
  message: string
  source: 'workflow' | 'config'
  field?: string
}

export type DispatchPreflightResult =
  | { ok: true }
  | { ok: false; errors: DispatchPreflightError[] }

export function isDispatchPreflightFailure(
  result: DispatchPreflightResult,
): result is Extract<DispatchPreflightResult, { ok: false }> {
  return result.ok === false
}
