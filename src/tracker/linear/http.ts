import {
  TrackerIntegrationError,
  createLinearApiRequestError,
  createLinearApiStatusError,
  createLinearGraphQLErrorsError,
} from '../errors.js'
import type { LinearGraphQLError } from './types.js'

export interface RunLinearGraphQLInput {
  endpoint: string
  apiKey: string
  query: string
  variables: Record<string, unknown>
  timeoutMs?: number
  fetchImpl?: typeof fetch
}

export async function runLinearGraphQL(
  input: RunLinearGraphQLInput,
): Promise<unknown> {
  const controller = new AbortController()
  const timeout = setTimeout(() => controller.abort(), input.timeoutMs ?? 30_000)
  const fetchImpl = input.fetchImpl ?? fetch

  try {
    const response = await fetchImpl(input.endpoint, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        authorization: input.apiKey,
      },
      body: JSON.stringify({ query: input.query, variables: input.variables }),
      signal: controller.signal,
    })

    if (!response.ok) {
      throw createLinearApiStatusError(response.status, response.statusText)
    }

    const payload = (await response.json()) as {
      data?: unknown
      errors?: LinearGraphQLError[]
    }

    if (Array.isArray(payload.errors) && payload.errors.length > 0) {
      throw createLinearGraphQLErrorsError(payload.errors)
    }

    return payload.data
  } catch (error: unknown) {
    if (error instanceof TrackerIntegrationError) {
      throw error
    }

    throw createLinearApiRequestError('Linear request failed', error)
  } finally {
    clearTimeout(timeout)
  }
}
