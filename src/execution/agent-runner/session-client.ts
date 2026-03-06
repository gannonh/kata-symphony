import { spawn, type ChildProcess, type ChildProcessWithoutNullStreams } from 'node:child_process'
import type { LiveSession } from '../../domain/models.js'
import {
  AGENT_RUNNER_ERROR_CODES,
  AgentRunnerError,
} from './errors.js'
import { createProtocolClient, type SessionStartResult } from './protocol-client.js'
import { createSessionReducer } from './session-reducer.js'
import { createStdioTransport } from './transport.js'

export interface AgentSessionStart extends SessionStartResult {}

export interface AgentSessionClient {
  startSession(input: { title: string; prompt: string }): Promise<AgentSessionStart>
  runTurn(input: { threadId: string; title: string; prompt: string }): Promise<AgentSessionStart>
  stopSession(): Promise<void>
  getLatestSession(): LiveSession | null
}

interface SessionClientDeps {
  codex: {
    command: string
    approval_policy?: string
    thread_sandbox?: string
    turn_sandbox_policy?: string
    turn_timeout_ms: number
    read_timeout_ms: number
    stall_timeout_ms: number
  }
  workspacePath: string
  spawnChild?: (input: {
    command: string
    cwd: string
  }) => ChildProcessWithoutNullStreams
}

interface ChildFailureSignal {
  failure: Promise<never>
  stop(): void
}

interface RuntimeState {
  child: ChildProcessWithoutNullStreams
  childFailure: ChildFailureSignal
  protocolClient: ReturnType<typeof createProtocolClient>
  sessionReducer: ReturnType<typeof createSessionReducer>
  stopTransport: () => void
  clearPending: () => void
  turnCount: number
}

function turnSandboxPolicy(value: string | undefined): { mode: string } | undefined {
  if (!value) {
    return undefined
  }

  return { mode: value }
}

function createChildFailureSignal(child: ChildProcess): ChildFailureSignal {
  let rejectFailure!: (error: AgentRunnerError) => void
  let settled = false

  const fail = () => {
    if (settled) {
      return
    }

    settled = true
    rejectFailure(new AgentRunnerError(AGENT_RUNNER_ERROR_CODES.RESPONSE_ERROR))
  }

  const failure = new Promise<never>((_resolve, reject) => {
    rejectFailure = reject
  })

  const onError = () => {
    fail()
  }
  const onExit = () => {
    fail()
  }
  const onClose = () => {
    fail()
  }

  child.once('error', onError)
  child.once('exit', onExit)
  child.once('close', onClose)

  return {
    failure,
    stop() {
      child.off('error', onError)
      child.off('exit', onExit)
      child.off('close', onClose)
    },
  }
}

export function createAgentSessionClient(deps: SessionClientDeps): AgentSessionClient {
  let runtime: RuntimeState | null = null
  let latestSession: LiveSession | null = null
  let latestStart: AgentSessionStart | null = null

  const ensureRuntime = () => {
    if (runtime) {
      return runtime
    }

    const child =
      deps.spawnChild?.({
        command: deps.codex.command,
        cwd: deps.workspacePath,
      }) ??
      spawn('bash', ['-lc', deps.codex.command], {
        cwd: deps.workspacePath,
        stdio: ['pipe', 'pipe', 'pipe'],
      })

    const pending = new Map<number, (value: unknown) => void>()
    const sessionReducer = createSessionReducer()
    const childFailure = createChildFailureSignal(child)
    const transport = createStdioTransport({
      child,
      onMessage(message) {
        if (typeof message.id === 'number') {
          pending.get(message.id)?.(message)
          pending.delete(message.id)
          return
        }

        sessionReducer.acceptMessage(message)
      },
    })

    runtime = {
      child,
      childFailure,
      protocolClient: createProtocolClient({
        readTimeoutMs: deps.codex.read_timeout_ms,
        sendLine: transport.sendLine,
        registerPending: (id, resolver) => {
          pending.set(id, resolver)
        },
        unregisterPending: (id) => {
          pending.delete(id)
        },
      }),
      sessionReducer,
      stopTransport: transport.stop,
      clearPending: () => {
        pending.clear()
      },
      turnCount: 0,
    }

    return runtime
  }

  const stopRuntime = async () => {
    if (!runtime) {
      latestSession = null
      latestStart = null
      return
    }

    runtime.childFailure.stop()
    runtime.clearPending()
    runtime.stopTransport()
    runtime.child.kill()
    runtime = null
    latestSession = null
    latestStart = null
  }

  return {
    async startSession(input) {
      if (latestStart) {
        throw new AgentRunnerError(AGENT_RUNNER_ERROR_CODES.SESSION_ALREADY_STARTED)
      }

      const activeRuntime = ensureRuntime()
      const sessionInput: {
        cwd: string
        title: string
        prompt: string
        approvalPolicy?: string
        threadSandbox?: string
        turnSandboxPolicy?: { mode: string }
      } = {
        cwd: deps.workspacePath,
        title: input.title,
        prompt: input.prompt,
      }
      if (deps.codex.approval_policy) {
        sessionInput.approvalPolicy = deps.codex.approval_policy
      }
      if (deps.codex.thread_sandbox) {
        sessionInput.threadSandbox = deps.codex.thread_sandbox
      }
      const initialSandboxPolicy = turnSandboxPolicy(deps.codex.turn_sandbox_policy)
      if (initialSandboxPolicy) {
        sessionInput.turnSandboxPolicy = initialSandboxPolicy
      }

      const sessionStart = await Promise.race([
        activeRuntime.protocolClient.startSession(sessionInput),
        activeRuntime.childFailure.failure,
      ])

      await Promise.race([
        activeRuntime.sessionReducer.waitForTurnCompletion(deps.codex.turn_timeout_ms),
        activeRuntime.childFailure.failure,
      ])

      activeRuntime.turnCount = 1
      latestStart = sessionStart
      latestSession = activeRuntime.sessionReducer.toLiveSession(
        sessionStart,
        activeRuntime.child.pid,
        activeRuntime.turnCount,
      )
      return sessionStart
    },

    async runTurn(input) {
      const activeRuntime = runtime
      if (!activeRuntime || !latestStart) {
        throw new AgentRunnerError(AGENT_RUNNER_ERROR_CODES.SESSION_NOT_STARTED)
      }

      activeRuntime.sessionReducer.resetForNextTurn()
      const nextTurnCount = activeRuntime.turnCount + 1
      const turnInput: {
        cwd: string
        threadId: string
        title: string
        prompt: string
        approvalPolicy?: string
        turnSandboxPolicy?: { mode: string }
      } = {
        cwd: deps.workspacePath,
        threadId: input.threadId,
        title: input.title,
        prompt: input.prompt,
      }
      if (deps.codex.approval_policy) {
        turnInput.approvalPolicy = deps.codex.approval_policy
      }
      const continuationSandboxPolicy = turnSandboxPolicy(deps.codex.turn_sandbox_policy)
      if (continuationSandboxPolicy) {
        turnInput.turnSandboxPolicy = continuationSandboxPolicy
      }

      const sessionStart = await Promise.race([
        activeRuntime.protocolClient.startTurn(turnInput),
        activeRuntime.childFailure.failure,
      ])

      await Promise.race([
        activeRuntime.sessionReducer.waitForTurnCompletion(deps.codex.turn_timeout_ms),
        activeRuntime.childFailure.failure,
      ])

      activeRuntime.turnCount = nextTurnCount
      latestStart = sessionStart
      latestSession = activeRuntime.sessionReducer.toLiveSession(
        sessionStart,
        activeRuntime.child.pid,
        activeRuntime.turnCount,
      )
      return sessionStart
    },

    async stopSession() {
      await stopRuntime()
    },

    getLatestSession() {
      return latestSession
    },
  }
}
