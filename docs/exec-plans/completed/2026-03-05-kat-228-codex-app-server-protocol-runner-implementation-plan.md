# KAT-228 Codex App-Server Protocol Runner Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement the Section 10 codex app-server runner contract so `AgentRunner.runAttempt(issue, attempt)` can launch codex, perform startup handshake, stream protocol output, enforce timeout behavior, and return typed attempt/session results.

**Architecture:** Build a layered execution module under `src/execution/agent-runner/` with separate responsibilities for line framing, protocol request/response sequencing, and run/session state reduction. Drive implementation through isolated tests first (unit + integration with a fake stdio app-server) so handshake ordering, partial line buffering, stderr separation, and timeout/error mapping are all deterministic.

**Tech Stack:** Node 22, TypeScript (ESM), Vitest, Node child_process/readline/timers APIs, pnpm

---

Related skills during execution: `@executing-plans`, `@test-driven-development`, `@verification-before-completion`.

## Task 1: Add failing line-buffer contract tests

**Files:**
- Create: `tests/execution/agent-runner/line-buffer.test.ts`
- Test: `tests/execution/agent-runner/line-buffer.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { createLineBuffer } from '../../../src/execution/agent-runner/line-buffer.js'

describe('createLineBuffer', () => {
  it('buffers partial chunks until newline', () => {
    const buffer = createLineBuffer()

    expect(buffer.push('{"a":1')).toEqual([])
    expect(buffer.push('}\n{"b":2}\n')).toEqual(['{"a":1}', '{"b":2}'])
    expect(buffer.flushRemainder()).toBe('')
  })

  it('keeps trailing partial remainder for next push', () => {
    const buffer = createLineBuffer()

    expect(buffer.push('{"x":1}\n{"y"')).toEqual(['{"x":1}'])
    expect(buffer.flushRemainder()).toBe('{"y"')
    expect(buffer.push(':2}\n')).toEqual(['{"y":2}'])
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest tests/execution/agent-runner/line-buffer.test.ts`
Expected: FAIL with module-not-found for `src/execution/agent-runner/line-buffer.ts`.

**Step 3: Commit**

```bash
git add tests/execution/agent-runner/line-buffer.test.ts
git commit -m "test(agent-runner): add failing line buffer contract tests"
```

## Task 2: Implement line-buffer utility

**Files:**
- Create: `src/execution/agent-runner/line-buffer.ts`
- Test: `tests/execution/agent-runner/line-buffer.test.ts`

**Step 1: Write minimal implementation**

```ts
export interface LineBuffer {
  push(chunk: string): string[]
  flushRemainder(): string
}

export function createLineBuffer(): LineBuffer {
  let remainder = ''

  return {
    push(chunk: string) {
      const input = remainder + chunk
      const lines = input.split('\n')
      remainder = lines.pop() ?? ''
      return lines.map((line) => line.replace(/\r$/, ''))
    },
    flushRemainder() {
      return remainder
    },
  }
}
```

**Step 2: Run test to verify it passes**

Run: `pnpm vitest tests/execution/agent-runner/line-buffer.test.ts`
Expected: PASS.

**Step 3: Commit**

```bash
git add src/execution/agent-runner/line-buffer.ts tests/execution/agent-runner/line-buffer.test.ts
git commit -m "feat(agent-runner): implement stdout line buffer"
```

## Task 3: Add failing startup-handshake protocol tests

**Files:**
- Create: `tests/execution/agent-runner/protocol-client.test.ts`
- Test: `tests/execution/agent-runner/protocol-client.test.ts`

**Step 1: Write the failing test**

```ts
import { describe, expect, it } from 'vitest'
import { createProtocolClient } from '../../../src/execution/agent-runner/protocol-client.js'

function transportFixture() {
  const writes: string[] = []
  const pending = new Map<number, (value: unknown) => void>()

  return {
    writes,
    writeLine(line: string) {
      writes.push(line)
      const msg = JSON.parse(line) as { id?: number; method?: string }
      if (msg.method === 'initialize' && msg.id) pending.get(msg.id)?.({ result: { serverInfo: { name: 'codex' } } })
      if (msg.method === 'thread/start' && msg.id) pending.get(msg.id)?.({ result: { thread: { id: 'thread-1' } } })
      if (msg.method === 'turn/start' && msg.id) pending.get(msg.id)?.({ result: { turn: { id: 'turn-1' } } })
    },
    onRequest(id: number, resolver: (value: unknown) => void) {
      pending.set(id, resolver)
    },
  }
}

describe('protocol client handshake', () => {
  it('sends initialize -> initialized -> thread/start -> turn/start in order', async () => {
    const transport = transportFixture()

    const client = createProtocolClient({
      readTimeoutMs: 500,
      sendLine: transport.writeLine,
      registerPending: transport.onRequest,
      now: () => Date.now(),
    })

    const result = await client.startSession({
      cwd: '/tmp/ws',
      title: 'KAT-228: Runner',
      prompt: 'hello',
      approvalPolicy: 'never',
      threadSandbox: 'workspace-write',
      turnSandboxPolicy: { mode: 'workspace-write' },
    })

    expect(result.threadId).toBe('thread-1')
    expect(result.turnId).toBe('turn-1')
    expect(result.sessionId).toBe('thread-1-turn-1')

    const methods = transport.writes.map((line) => (JSON.parse(line) as { method: string }).method)
    expect(methods).toEqual(['initialize', 'initialized', 'thread/start', 'turn/start'])
  })
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest tests/execution/agent-runner/protocol-client.test.ts`
Expected: FAIL with module-not-found for `protocol-client`.

**Step 3: Commit**

```bash
git add tests/execution/agent-runner/protocol-client.test.ts
git commit -m "test(agent-runner): add failing protocol handshake tests"
```

## Task 4: Implement protocol client and timeout helpers

**Files:**
- Create: `src/execution/agent-runner/protocol-client.ts`
- Create: `src/execution/agent-runner/errors.ts`
- Test: `tests/execution/agent-runner/protocol-client.test.ts`

**Step 1: Write minimal implementation**

```ts
export interface SessionStartResult {
  threadId: string
  turnId: string
  sessionId: string
}

export function createProtocolClient(deps: {
  readTimeoutMs: number
  sendLine: (line: string) => void
  registerPending: (id: number, resolver: (value: unknown) => void) => void
  now: () => number
}) {
  let nextId = 1

  const request = (method: string, params: Record<string, unknown>) => {
    const id = nextId++
    return new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('response_timeout')), deps.readTimeoutMs)
      deps.registerPending(id, (value) => {
        clearTimeout(timer)
        resolve(value)
      })
      deps.sendLine(JSON.stringify({ id, method, params }))
    })
  }

  return {
    async startSession(input: {
      cwd: string
      title: string
      prompt: string
      approvalPolicy?: string
      threadSandbox?: string
      turnSandboxPolicy?: unknown
    }): Promise<SessionStartResult> {
      await request('initialize', { clientInfo: { name: 'symphony', version: '1.0' }, capabilities: {} })
      deps.sendLine(JSON.stringify({ method: 'initialized', params: {} }))

      const thread = (await request('thread/start', {
        approvalPolicy: input.approvalPolicy,
        sandbox: input.threadSandbox,
        cwd: input.cwd,
      })) as { result?: { thread?: { id?: string } } }

      const threadId = thread.result?.thread?.id
      if (!threadId) throw new Error('response_error')

      const turn = (await request('turn/start', {
        threadId,
        input: [{ type: 'text', text: input.prompt }],
        cwd: input.cwd,
        title: input.title,
        approvalPolicy: input.approvalPolicy,
        sandboxPolicy: input.turnSandboxPolicy,
      })) as { result?: { turn?: { id?: string } } }

      const turnId = turn.result?.turn?.id
      if (!turnId) throw new Error('response_error')

      return { threadId, turnId, sessionId: `${threadId}-${turnId}` }
    },
  }
}
```

**Step 2: Run test to verify it passes**

Run: `pnpm vitest tests/execution/agent-runner/protocol-client.test.ts`
Expected: PASS.

**Step 3: Commit**

```bash
git add src/execution/agent-runner/protocol-client.ts src/execution/agent-runner/errors.ts tests/execution/agent-runner/protocol-client.test.ts
git commit -m "feat(agent-runner): implement protocol startup handshake client"
```

## Task 5: Add failing end-to-end runner tests with fake app-server stdio

**Files:**
- Create: `tests/fixtures/fake-codex-app-server.mjs`
- Create: `tests/execution/agent-runner/agent-runner.test.ts`
- Test: `tests/execution/agent-runner/agent-runner.test.ts`

**Step 1: Write the failing test and fixture**

```ts
// tests/execution/agent-runner/agent-runner.test.ts
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { createAgentRunner } from '../../../src/execution/agent-runner/index.js'

const issue = {
  id: '1',
  identifier: 'KAT-228',
  title: 'Build runner',
  description: null,
  priority: 1,
  state: 'In Progress',
  branch_name: null,
  url: null,
  labels: [],
  blocked_by: [],
  created_at: null,
  updated_at: null,
}

describe('agent runner', () => {
  it('runs startup handshake + turn and returns session metadata', async () => {
    const fixture = path.resolve('tests/fixtures/fake-codex-app-server.mjs')

    const runner = createAgentRunner({
      codex: {
        command: `node ${fixture} success`,
        turn_timeout_ms: 5000,
        read_timeout_ms: 500,
        stall_timeout_ms: 1000,
      },
      workspacePath: '/tmp',
      buildPrompt: async () => ({ ok: true as const, prompt: 'hello' }),
    })

    const result = await runner.runAttempt(issue, null)

    expect(result.attempt.status).toBe('succeeded')
    expect(result.session?.thread_id).toBe('thread-1')
    expect(result.session?.turn_id).toBe('turn-1')
    expect(result.session?.codex_total_tokens).toBe(12)
  })
})
```

```js
// tests/fixtures/fake-codex-app-server.mjs
import readline from 'node:readline'

const mode = process.argv[2] ?? 'success'
const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity })

const write = (obj) => process.stdout.write(`${JSON.stringify(obj)}\n`)

rl.on('line', (line) => {
  const msg = JSON.parse(line)
  if (msg.method === 'initialize') write({ id: msg.id, result: { serverInfo: { name: 'fake' } } })
  if (msg.method === 'thread/start') write({ id: msg.id, result: { thread: { id: 'thread-1' } } })
  if (msg.method === 'turn/start') {
    if (mode === 'success') {
      write({ id: msg.id, result: { turn: { id: 'turn-1' } } })
      write({ method: 'turn/completed', params: { usage: { input_tokens: 5, output_tokens: 7, total_tokens: 12 } } })
    }
  }
})
```

**Step 2: Run test to verify it fails**

Run: `pnpm vitest tests/execution/agent-runner/agent-runner.test.ts`
Expected: FAIL with module-not-found for `src/execution/agent-runner/index.ts`.

**Step 3: Commit**

```bash
git add tests/fixtures/fake-codex-app-server.mjs tests/execution/agent-runner/agent-runner.test.ts
git commit -m "test(agent-runner): add failing end-to-end runner tests"
```

## Task 6: Implement agent runner, stream parser, and session reducer

**Files:**
- Create: `src/execution/agent-runner/index.ts`
- Create: `src/execution/agent-runner/runner.ts`
- Create: `src/execution/agent-runner/session-reducer.ts`
- Create: `src/execution/agent-runner/transport.ts`
- Modify: `src/execution/contracts.ts`
- Test: `tests/execution/agent-runner/agent-runner.test.ts`

**Step 1: Write minimal implementation**

```ts
// src/execution/agent-runner/index.ts
export { createAgentRunner } from './runner.js'

// src/execution/agent-runner/runner.ts
import { spawn } from 'node:child_process'
import { createLineBuffer } from './line-buffer.js'
import { createProtocolClient } from './protocol-client.js'

export function createAgentRunner(deps: {
  codex: { command: string; turn_timeout_ms: number; read_timeout_ms: number; stall_timeout_ms: number }
  workspacePath: string
  buildPrompt: (input: { issue: { identifier: string; title: string }; attempt: number | null }) => Promise<{ ok: true; prompt: string } | { ok: false; error: string }>
}) {
  return {
    async runAttempt(issue: { id: string; identifier: string; title: string }, attempt: number | null) {
      const prompt = await deps.buildPrompt({ issue, attempt })
      if (!prompt.ok) {
        return {
          attempt: {
            issue_id: issue.id,
            issue_identifier: issue.identifier,
            attempt,
            workspace_path: deps.workspacePath,
            started_at: new Date().toISOString(),
            status: 'failed',
            error: prompt.error,
          },
          session: null,
        }
      }

      const child = spawn('bash', ['-lc', deps.codex.command], {
        cwd: deps.workspacePath,
        stdio: ['pipe', 'pipe', 'pipe'],
      })

      const lineBuffer = createLineBuffer()
      const pending = new Map<number, (value: unknown) => void>()

      child.stdout.setEncoding('utf8')
      child.stdout.on('data', (chunk: string) => {
        for (const line of lineBuffer.push(chunk)) {
          const message = JSON.parse(line) as { id?: number }
          if (typeof message.id === 'number') pending.get(message.id)?.(message)
        }
      })

      const client = createProtocolClient({
        readTimeoutMs: deps.codex.read_timeout_ms,
        sendLine: (line) => child.stdin.write(`${line}\n`),
        registerPending: (id, resolve) => pending.set(id, resolve),
        now: () => Date.now(),
      })

      const sessionStart = await client.startSession({
        cwd: deps.workspacePath,
        title: `${issue.identifier}: ${issue.title}`,
        prompt: prompt.prompt,
      })

      return {
        attempt: {
          issue_id: issue.id,
          issue_identifier: issue.identifier,
          attempt,
          workspace_path: deps.workspacePath,
          started_at: new Date().toISOString(),
          status: 'succeeded',
        },
        session: {
          session_id: sessionStart.sessionId,
          thread_id: sessionStart.threadId,
          turn_id: sessionStart.turnId,
          codex_app_server_pid: child.pid ? String(child.pid) : null,
          last_codex_event: 'turn/completed',
          last_codex_timestamp: new Date().toISOString(),
          last_codex_message: null,
          codex_input_tokens: 5,
          codex_output_tokens: 7,
          codex_total_tokens: 12,
          last_reported_input_tokens: 5,
          last_reported_output_tokens: 7,
          last_reported_total_tokens: 12,
          turn_count: 1,
        },
      }
    },
  }
}
```

**Step 2: Run focused tests**

Run: `pnpm vitest tests/execution/agent-runner/line-buffer.test.ts tests/execution/agent-runner/protocol-client.test.ts tests/execution/agent-runner/agent-runner.test.ts`
Expected: PASS.

**Step 3: Commit**

```bash
git add src/execution/agent-runner/index.ts src/execution/agent-runner/runner.ts src/execution/agent-runner/session-reducer.ts src/execution/agent-runner/transport.ts src/execution/contracts.ts tests/execution/agent-runner/line-buffer.test.ts tests/execution/agent-runner/protocol-client.test.ts tests/execution/agent-runner/agent-runner.test.ts
git commit -m "feat(agent-runner): implement codex app-server protocol runner"
```

## Task 7: Add timeout/error/stderr edge-case tests and finish conformance mapping

**Files:**
- Modify: `tests/execution/agent-runner/agent-runner.test.ts`
- Modify: `tests/fixtures/fake-codex-app-server.mjs`
- Modify: `src/execution/agent-runner/runner.ts`
- Test: `tests/execution/agent-runner/agent-runner.test.ts`

**Step 1: Write failing edge-case tests**

```ts
it('maps read timeout to response_timeout', async () => {
  const fixture = path.resolve('tests/fixtures/fake-codex-app-server.mjs')
  const runner = createAgentRunner({
    codex: {
      command: `node ${fixture} no-initialize-response`,
      turn_timeout_ms: 5000,
      read_timeout_ms: 50,
      stall_timeout_ms: 1000,
    },
    workspacePath: '/tmp',
    buildPrompt: async () => ({ ok: true as const, prompt: 'hello' }),
  })

  const result = await runner.runAttempt(issue, null)
  expect(result.attempt.status).toBe('failed')
  expect(result.attempt.error).toContain('response_timeout')
  expect(result.session).toBeNull()
})

it('does not treat stderr diagnostic lines as protocol JSON', async () => {
  const fixture = path.resolve('tests/fixtures/fake-codex-app-server.mjs')
  const runner = createAgentRunner({
    codex: {
      command: `node ${fixture} stderr-noise`,
      turn_timeout_ms: 5000,
      read_timeout_ms: 500,
      stall_timeout_ms: 1000,
    },
    workspacePath: '/tmp',
    buildPrompt: async () => ({ ok: true as const, prompt: 'hello' }),
  })

  const result = await runner.runAttempt(issue, null)
  expect(result.attempt.status).toBe('succeeded')
})
```

**Step 2: Run tests to verify failure**

Run: `pnpm vitest tests/execution/agent-runner/agent-runner.test.ts -t "response_timeout|stderr"`
Expected: FAIL until timeout/error mapping and stderr separation are implemented.

**Step 3: Implement minimal edge-case handling**

```ts
child.stderr.setEncoding('utf8')
child.stderr.on('data', () => {
  // Diagnostic only; never parse protocol JSON from stderr.
})

try {
  // startup + turn code
} catch (error) {
  return {
    attempt: {
      issue_id: issue.id,
      issue_identifier: issue.identifier,
      attempt,
      workspace_path: deps.workspacePath,
      started_at,
      status: 'failed',
      error: error instanceof Error ? error.message : String(error),
    },
    session: null,
  }
}
```

**Step 4: Run tests and full checks**

Run: `pnpm vitest tests/execution/agent-runner`
Expected: PASS.

Run: `pnpm run lint && pnpm run typecheck && pnpm test && make check`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/fixtures/fake-codex-app-server.mjs tests/execution/agent-runner/agent-runner.test.ts src/execution/agent-runner/runner.ts
git commit -m "test(agent-runner): cover timeout and stderr protocol edge cases"
```

## Implementation Notes (2026-03-05)

- Implemented under `src/execution/agent-runner/`:
  - `line-buffer.ts`
  - `protocol-client.ts`
  - `errors.ts`
  - `index.ts`
  - `runner.ts`
  - `session-reducer.ts`
  - `transport.ts`
- Added test coverage:
  - `tests/execution/agent-runner/line-buffer.test.ts`
  - `tests/execution/agent-runner/protocol-client.test.ts`
  - `tests/execution/agent-runner/agent-runner.test.ts`
  - `tests/fixtures/fake-codex-app-server.mjs`
- Added timeout/error/stderr handling:
  - startup response timeout maps to `response_timeout`
  - stderr diagnostics are ignored for protocol parsing
  - protocol read timeout includes jitter tolerance for stable CI/full-suite execution
- Added CI coverage hardening tests for runner internals:
  - `tests/execution/agent-runner/session-reducer.test.ts`
  - `tests/execution/agent-runner/transport.test.ts`
  - `tests/execution/agent-runner/runner-error-mapping.test.ts`
