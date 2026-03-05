import { startService } from './service.js'

export async function runMain(
  start: () => Promise<void> = startService,
  onError: (message: string, error: unknown) => void = (message, error) => {
    console.error(message, error)
  },
): Promise<void> {
  try {
    await start()
  } catch (error) {
    process.exitCode = 1
    try {
      onError('Symphony startup failed', error)
    } catch {
      // Preserve startup failure exit semantics even if logging/reporting fails.
    }
  }
}
