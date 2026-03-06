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
