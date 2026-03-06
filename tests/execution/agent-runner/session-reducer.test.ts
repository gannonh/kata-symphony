import { describe, expect, it } from 'vitest'
import { createSessionReducer } from '../../../src/execution/agent-runner/session-reducer.js'

describe('session reducer', () => {
  it('ignores invalid messages and returns default session snapshot', () => {
    const reducer = createSessionReducer()

    reducer.acceptMessage('not-an-object')
    reducer.acceptMessage({})

    const session = reducer.toLiveSession(
      { threadId: 'thread-1', turnId: 'turn-1', sessionId: 'thread-1-turn-1' },
      undefined,
      1,
    )

    expect(session.codex_app_server_pid).toBeNull()
    expect(session.last_codex_event).toBeNull()
    expect(session.last_codex_message).toBeNull()
    expect(session.codex_total_tokens).toBe(0)
    expect(session.turn_count).toBe(1)
  })

  it('times out when no turn/completed event arrives', async () => {
    const reducer = createSessionReducer()
    reducer.acceptMessage({ method: 'turn/started', params: {} })

    await expect(reducer.waitForTurnCompletion(5)).rejects.toThrow('response_timeout')
  })

  it('captures usage values on turn/completed', async () => {
    const reducer = createSessionReducer()
    reducer.acceptMessage({
      method: 'turn/completed',
      params: { usage: { input_tokens: 5, output_tokens: 7, total_tokens: 12 } },
    })

    await expect(reducer.waitForTurnCompletion(5)).resolves.toBeUndefined()

    const session = reducer.toLiveSession(
      { threadId: 'thread-1', turnId: 'turn-1', sessionId: 'thread-1-turn-1' },
      42,
      1,
    )
    expect(session.codex_app_server_pid).toBe('42')
    expect(session.last_codex_event).toBe('turn/completed')
    expect(session.codex_input_tokens).toBe(5)
    expect(session.codex_output_tokens).toBe(7)
    expect(session.codex_total_tokens).toBe(12)
  })

  it('resets completion state and increments turn count for the next turn', async () => {
    const reducer = createSessionReducer()
    reducer.acceptMessage({
      method: 'turn/completed',
      params: { usage: { input_tokens: 1, output_tokens: 2, total_tokens: 3 } },
    })

    await expect(reducer.waitForTurnCompletion(5)).resolves.toBeUndefined()
    reducer.resetForNextTurn()

    const session = reducer.toLiveSession(
      { threadId: 'thread-1', turnId: 'turn-2', sessionId: 'thread-1-turn-2' },
      42,
      2,
    )
    expect(session.turn_count).toBe(2)
    expect(session.turn_id).toBe('turn-2')

    const waiting = reducer.waitForTurnCompletion(50)
    setTimeout(() => {
      reducer.acceptMessage({
        method: 'turn/completed',
        params: { usage: { input_tokens: 4, output_tokens: 5, total_tokens: 9 } },
      })
    }, 0)

    await expect(waiting).resolves.toBeUndefined()
    const completedSession = reducer.toLiveSession(
      { threadId: 'thread-1', turnId: 'turn-2', sessionId: 'thread-1-turn-2' },
      42,
      2,
    )
    expect(completedSession.codex_total_tokens).toBe(9)
    expect(completedSession.turn_count).toBe(2)
  })

  it('falls back to zero usage when usage payload is malformed', async () => {
    const reducer = createSessionReducer()
    reducer.acceptMessage({
      method: 'turn/completed',
      params: { usage: { input_tokens: '5', output_tokens: 7 } },
    })

    await expect(reducer.waitForTurnCompletion(5)).resolves.toBeUndefined()
    const session = reducer.toLiveSession(
      { threadId: 'thread-1', turnId: 'turn-1', sessionId: 'thread-1-turn-1' },
      1,
      1,
    )

    expect(session.codex_input_tokens).toBe(0)
    expect(session.codex_output_tokens).toBe(7)
    expect(session.codex_total_tokens).toBe(0)
  })

  it('falls back to zero for non-numeric output tokens', async () => {
    const reducer = createSessionReducer()
    reducer.acceptMessage({
      method: 'turn/completed',
      params: { usage: { input_tokens: 3, output_tokens: 'bad', total_tokens: 8 } },
    })

    await expect(reducer.waitForTurnCompletion(5)).resolves.toBeUndefined()
    const session = reducer.toLiveSession(
      { threadId: 'thread-1', turnId: 'turn-1', sessionId: 'thread-1-turn-1' },
      1,
      1,
    )

    expect(session.codex_input_tokens).toBe(3)
    expect(session.codex_output_tokens).toBe(0)
    expect(session.codex_total_tokens).toBe(8)
  })

  it('falls back to zero usage when params is non-object', async () => {
    const reducer = createSessionReducer()
    reducer.acceptMessage({
      method: 'turn/completed',
      params: 'bad-payload',
    })

    await expect(reducer.waitForTurnCompletion(5)).resolves.toBeUndefined()
    const session = reducer.toLiveSession(
      { threadId: 'thread-1', turnId: 'turn-1', sessionId: 'thread-1-turn-1' },
      1,
      1,
    )

    expect(session.codex_input_tokens).toBe(0)
    expect(session.codex_output_tokens).toBe(0)
    expect(session.codex_total_tokens).toBe(0)
  })

  it('falls back to zero usage when usage object is absent', async () => {
    const reducer = createSessionReducer()
    reducer.acceptMessage({
      method: 'turn/completed',
      params: {},
    })

    await expect(reducer.waitForTurnCompletion(5)).resolves.toBeUndefined()
    const session = reducer.toLiveSession(
      { threadId: 'thread-1', turnId: 'turn-1', sessionId: 'thread-1-turn-1' },
      1,
      1,
    )

    expect(session.codex_input_tokens).toBe(0)
    expect(session.codex_output_tokens).toBe(0)
    expect(session.codex_total_tokens).toBe(0)
  })

  it('resolves immediately on turn/failed', async () => {
    const reducer = createSessionReducer()
    reducer.acceptMessage({ method: 'turn/failed', params: {} })

    await expect(reducer.waitForTurnCompletion(5)).resolves.toBeUndefined()
    const session = reducer.toLiveSession(
      { threadId: 'thread-1', turnId: 'turn-1', sessionId: 'thread-1-turn-1' },
      1,
      1,
    )
    expect(session.last_codex_event).toBe('turn/failed')
  })

  it('resolves immediately on turn/cancelled', async () => {
    const reducer = createSessionReducer()
    reducer.acceptMessage({ method: 'turn/cancelled', params: {} })

    await expect(reducer.waitForTurnCompletion(5)).resolves.toBeUndefined()
    const session = reducer.toLiveSession(
      { threadId: 'thread-1', turnId: 'turn-1', sessionId: 'thread-1-turn-1' },
      1,
      1,
    )
    expect(session.last_codex_event).toBe('turn/cancelled')
  })

  it('resolves when completion arrives after waiting has started', async () => {
    const reducer = createSessionReducer()
    const waiting = reducer.waitForTurnCompletion(50)

    setTimeout(() => {
      reducer.acceptMessage({
        method: 'turn/completed',
        params: { usage: { input_tokens: 1, output_tokens: 2, total_tokens: 3 } },
      })
    }, 0)

    await expect(waiting).resolves.toBeUndefined()
  })
})
