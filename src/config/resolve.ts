import path from 'node:path'

const TOKEN_RE = /^\$([A-Z0-9_]+)$/

export function resolveEnvToken(value: string, env: NodeJS.ProcessEnv): string {
  const match = value.match(TOKEN_RE)
  if (!match) {
    return value
  }

  const token = match[1]
  if (!token) {
    return ''
  }

  return env[token] ?? ''
}

export function resolvePathValue(value: string, env: NodeJS.ProcessEnv): string {
  const resolved = resolveEnvToken(value, env)

  if (resolved === '~') {
    return env.HOME ?? resolved
  }

  if (resolved.startsWith('~/')) {
    const home = env.HOME ?? ''
    if (!home) {
      return resolved
    }

    return path.join(home, resolved.slice(2))
  }

  return resolved
}
