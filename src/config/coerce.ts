function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

export function coerceInteger(value: unknown, fallback: number): number {
  if (typeof value === 'number' && Number.isInteger(value)) {
    return value
  }

  if (typeof value === 'string') {
    const parsed = Number(value.trim())
    if (Number.isInteger(parsed)) {
      return parsed
    }
  }

  return fallback
}

export function coerceStringList(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value
      .filter((entry): entry is string => typeof entry === 'string')
      .map((entry) => entry.trim())
      .filter(Boolean)
  }

  if (typeof value === 'string') {
    return value
      .split(',')
      .map((entry) => entry.trim())
      .filter(Boolean)
  }

  return []
}

export function normalizeStateMap(value: unknown): Record<string, number> {
  if (!isRecord(value)) {
    return {}
  }

  const normalized: Record<string, number> = {}
  for (const [rawState, rawLimit] of Object.entries(value)) {
    const state = rawState.trim().toLowerCase()
    const limit = coerceInteger(rawLimit, 0)
    if (state && limit > 0) {
      normalized[state] = limit
    }
  }

  return normalized
}
