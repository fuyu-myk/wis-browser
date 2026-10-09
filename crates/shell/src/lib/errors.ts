/**
 * The readable part of a rejected command. Tauri rejects with the command's
 * error string, or with a plain object carrying `message`, so `String(e)` alone
 * would show "[object Object]".
 */
export function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (
    error !== null &&
    typeof error === 'object' &&
    'message' in error &&
    typeof (error as { message: unknown }).message === 'string'
  ) {
    return (error as { message: string }).message
  }
  return 'unknown error'
}
