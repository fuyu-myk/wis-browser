/**
 * Text files attached to a canvas agent request: read in the page, checked
 * here, and sent fenced as data with the request they came with.
 */

export interface Attachment {
  name: string
  text: string
}

export type Accepted = { ok: true; file: Attachment } | { ok: false; reason: string }

export const MAX_FILE_CHARS = 200_000
export const MAX_TOTAL_CHARS = 500_000
export const MAX_FILES = 10
const MAX_NAME = 120

const UNSAFE_NAME = /["<>\u0000-\u001f\u007f\u200b-\u200f\u2028\u2029\u202a-\u202e\u2066-\u2069]/g

const tidyName = (name: string) => name.replace(UNSAFE_NAME, '').trim().slice(0, MAX_NAME) || 'file'

export function acceptFile(name: string, text: string, taken: readonly Attachment[]): Accepted {
  const clean = tidyName(name)
  if (taken.length >= MAX_FILES) return { ok: false, reason: `${clean}: at most ${MAX_FILES} files at once` }
  if (taken.some((f) => f.name === clean)) return { ok: false, reason: `${clean} is already attached` }
  if (text.includes('\u0000')) return { ok: false, reason: `${clean} is not a text file` }
  if (text.trim() === '') return { ok: false, reason: `${clean} is empty` }
  if (text.length > MAX_FILE_CHARS) return { ok: false, reason: `${clean} is too large` }
  const total = taken.reduce((sum, f) => sum + f.text.length, 0)
  if (total + text.length > MAX_TOTAL_CHARS) return { ok: false, reason: `${clean}: too much attached text in one request` }
  return { ok: true, file: { name: clean, text } }
}

export function attachmentBlock(files: readonly Attachment[]): string {
  return files
    .map((f) => `<attachment name="${tidyName(f.name)}">\n${f.text.replace(/<(\/?attachment)/gi, '&lt;$1')}\n</attachment>`)
    .join('\n')
}
