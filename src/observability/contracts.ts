/* v8 ignore file -- type-only contract surface */
/* c8 ignore file -- type-only contract surface */
export interface Logger {
  info(message: string, context?: Record<string, unknown>): void
  error(message: string, context?: Record<string, unknown>): void
}
