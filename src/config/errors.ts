export class ConfigValidationError extends Error {
  readonly code: string
  readonly path: string

  constructor(code: string, path: string, message: string) {
    super(message)
    this.name = 'ConfigValidationError'
    this.code = code
    this.path = path
  }
}
