/**
 * Spoken prompts on the canvas: Rust runs on-device speech recognition and
 * pushes what it hears; the prompt shows it after whatever was typed.
 */

export type MicEvent =
  | { type: 'heard'; text: string; done: boolean }
  | { type: 'failed'; message: string }
  | { type: 'ended' }

/** Longer than any prompt needs; past it the words are dropped. */
export const MAX_HEARD = 20_000

export function readMicEvent(raw: unknown): MicEvent | null {
  if (typeof raw !== 'object' || raw === null) return null
  const event = raw as Record<string, unknown>
  if (event.type === 'heard' && typeof event.text === 'string' && typeof event.done === 'boolean')
    return { type: 'heard', text: event.text.slice(0, MAX_HEARD), done: event.done }
  if (event.type === 'failed' && typeof event.message === 'string') return { type: 'failed', message: event.message }
  if (event.type === 'ended') return { type: 'ended' }
  return null
}

/** The prompt while dictating: what was typed, then what was heard. */
export function dictated(typed: string, heard: string): string {
  const base = typed.replace(/[ \t]+$/, '')
  if (heard === '') return base
  if (base === '' || base.endsWith('\n')) return base + heard
  return `${base} ${heard}`
}
