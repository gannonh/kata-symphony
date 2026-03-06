import readline from 'node:readline'

const mode = process.argv[2] ?? 'success'
const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity })

const write = (obj) => process.stdout.write(`${JSON.stringify(obj)}\n`)

rl.on('line', (line) => {
  let msg
  try {
    msg = JSON.parse(line)
  } catch {
    return
  }
  if (msg.method === 'initialize') {
    if (mode !== 'no-initialize-response') {
      write({ id: msg.id, result: { serverInfo: { name: 'fake' } } })
    }
  }
  if (msg.method === 'thread/start') write({ id: msg.id, result: { thread: { id: 'thread-1' } } })
  if (msg.method === 'turn/start') {
    if (mode === 'success') {
      write({ id: msg.id, result: { turn: { id: 'turn-1' } } })
      write({
        method: 'turn/completed',
        params: { usage: { input_tokens: 5, output_tokens: 7, total_tokens: 12 } },
      })
      return
    }

    if (mode === 'stderr-noise') {
      process.stderr.write('diagnostic: non-json stderr noise\n')
      write({ id: msg.id, result: { turn: { id: 'turn-1' } } })
      write({
        method: 'turn/completed',
        params: { usage: { input_tokens: 5, output_tokens: 7, total_tokens: 12 } },
      })
    }
  }
})
