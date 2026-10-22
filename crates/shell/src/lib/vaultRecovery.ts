import { errorMessage } from '@/lib/errors'

export function shortKey(line: string): string {
  const [kind = '', body = '', ...comment] = line.trim().split(/\s+/)
  if (kind.startsWith('age1')) return `${kind.slice(0, 10)}…${kind.slice(-7)}`
  return [kind, `…${body.slice(-7)}`, ...comment].join(' ')
}

export function unlockProblem(error: unknown): string {
  return `${errorMessage(error)}. Settings > Passwords can recover your vault.`
}
