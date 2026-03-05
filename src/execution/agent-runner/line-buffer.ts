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
