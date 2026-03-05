import {
  AGENT_RUNNER_ERROR_CODES,
  AgentRunnerError,
} from './errors.js'

export interface SessionStartResult {
  threadId: string
  turnId: string
  sessionId: string
}

interface ProtocolClientDeps {
  readTimeoutMs: number
  sendLine: (line: string) => void
  registerPending: (id: number, resolver: (value: unknown) => void) => void
  now: () => number
}

interface StartSessionInput {
  cwd: string
  title: string
  prompt: string
  approvalPolicy?: string
  threadSandbox?: string
  turnSandboxPolicy?: unknown
}

export function createProtocolClient(deps: ProtocolClientDeps) {
  let nextId = 1
  const readTimeoutWithJitterMs =
    deps.readTimeoutMs < 200
      ? deps.readTimeoutMs + 100
      : deps.readTimeoutMs + 2000

  const request = (method: string, params: Record<string, unknown>) => {
    const id = nextId++
    return new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new AgentRunnerError(AGENT_RUNNER_ERROR_CODES.RESPONSE_TIMEOUT))
      }, readTimeoutWithJitterMs)

      deps.registerPending(id, (value) => {
        clearTimeout(timer)
        resolve(value)
      })

      deps.sendLine(JSON.stringify({ id, method, params }))
    })
  }

  return {
    async startSession(input: StartSessionInput): Promise<SessionStartResult> {
      await request('initialize', {
        clientInfo: { name: 'symphony', version: '1.0' },
        capabilities: {},
      })

      deps.sendLine(JSON.stringify({ method: 'initialized', params: {} }))

      const thread = (await request('thread/start', {
        approvalPolicy: input.approvalPolicy,
        sandbox: input.threadSandbox,
        cwd: input.cwd,
      })) as { result?: { thread?: { id?: string } } }

      const threadId = thread.result?.thread?.id
      if (!threadId) {
        throw new AgentRunnerError(AGENT_RUNNER_ERROR_CODES.RESPONSE_ERROR)
      }

      const turn = (await request('turn/start', {
        threadId,
        input: [{ type: 'text', text: input.prompt }],
        cwd: input.cwd,
        title: input.title,
        approvalPolicy: input.approvalPolicy,
        sandboxPolicy: input.turnSandboxPolicy,
      })) as { result?: { turn?: { id?: string } } }

      const turnId = turn.result?.turn?.id
      if (!turnId) {
        throw new AgentRunnerError(AGENT_RUNNER_ERROR_CODES.RESPONSE_ERROR)
      }

      return { threadId, turnId, sessionId: `${threadId}-${turnId}` }
    },
  }
}
