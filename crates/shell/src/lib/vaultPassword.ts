export const MIN_VAULT_PASSWORD_CHARS = 12

export function newPasswordProblem(password: string, repeat: string): string | null {
  if ([...password].length < MIN_VAULT_PASSWORD_CHARS) {
    return `Use at least ${MIN_VAULT_PASSWORD_CHARS} characters`
  }
  if (password !== repeat) return "The passwords don't match"
  return null
}
