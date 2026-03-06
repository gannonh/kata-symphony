import {
  AGENT_RUNNER_ERROR_CODES,
  AgentRunnerError,
} from './errors.js'

export interface SessionStartResult {
  threadId: string
  turnId: string
  sessionId: string
}

export interface ThreadStartResult {
  threadId: string
}

interface ProtocolClientDeps {
  readTimeoutMs: number
  sendLine: (line: string) => void
  registerPending: (id: number, resolver: (value: unknown) => void) => void
  unregisterPending?: (id: number) => void
}

interface StartSessionInput {
  cwd: string
  title: string
  prompt: string
  approvalPolicy?: string
  threadSandbox?: string
  turnSandboxPolicy?: unknown
}

interface StartThreadInput {
  cwd: string
  approvalPolicy?: string
  threadSandbox?: string
}

interface StartTurnInput {
  cwd: string
  threadId: string
  title: string
  prompt: string
  approvalPolicy?: string
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
        deps.unregisterPending?.(id)
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
    async initializeSession(): Promise<void> {
      await request('initialize', {
        clientInfo: { name: 'symphony', version: '1.0' },
        capabilities: {},
      })

      deps.sendLine(JSON.stringify({ method: 'initialized', params: {} }))
    },

    async startThread(input: StartThreadInput): Promise<ThreadStartResult> {
      const thread = (await request('thread/start', {
        approvalPolicy: input.approvalPolicy,
        sandbox: input.threadSandbox,
        cwd: input.cwd,
      })) as { result?: { thread?: { id?: string } } }

      const threadId = thread.result?.thread?.id
      if (!threadId) {
        throw new AgentRunnerError(AGENT_RUNNER_ERROR_CODES.RESPONSE_ERROR)
      }

      return { threadId }
    },

    async startTurn(input: StartTurnInput): Promise<SessionStartResult> {
      const turn = (await request('turn/start', {
        threadId: input.threadId,
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

      return { threadId: input.threadId, turnId, sessionId: `${input.threadId}-${turnId}` }
    },

    async startSession(input: StartSessionInput): Promise<SessionStartResult> {
      await this.initializeSession()
      const thread = await this.startThread({
        cwd: input.cwd,
        approvalPolicy: input.approvalPolicy,
        threadSandbox: input.threadSandbox,
      })

      return this.startTurn({
        cwd: input.cwd,
        threadId: thread.threadId,
        title: input.title,
        prompt: input.prompt,
        approvalPolicy: input.approvalPolicy,
        turnSandboxPolicy: input.turnSandboxPolicy,
      })
    },
  }
}
