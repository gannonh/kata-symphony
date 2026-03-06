export const AGENT_RUNNER_ERROR_CODES = {
  RESPONSE_TIMEOUT: 'response_timeout',
  RESPONSE_ERROR: 'response_error',
  SESSION_NOT_STARTED: 'session_not_started',
  SESSION_ALREADY_STARTED: 'session_already_started',
} as const

export type AgentRunnerErrorCode =
  (typeof AGENT_RUNNER_ERROR_CODES)[keyof typeof AGENT_RUNNER_ERROR_CODES]

export class AgentRunnerError extends Error {
  readonly code: AgentRunnerErrorCode

  constructor(code: AgentRunnerErrorCode) {
    super(code)
    this.name = 'AgentRunnerError'
    this.code = code
  }
}
