/**
 * Files attached to a canvas agent request: text files read in the page,
 * PDF and word processor files read by Rust, all checked here and sent fenced
 * as data with the request they came with. A file too long for the room left
 * keeps its start and says how much of it that is.
 */

/** How much of a file was read: pages of a PDF, else a share of its length. */
export interface Partial {
  read: number
  total: number
  pages: boolean
}

export interface Attachment {
  name: string
  text: string
  partial?: Partial
}

export type Accepted = { ok: true; file: Attachment } | { ok: false; reason: string }

export type DocumentKind = 'pdf' | 'docx' | 'doc' | 'rtf' | 'odt'

export type DocumentOutcome =
  | { status: 'read'; text: string; partial: Partial | null }
  | { status: 'locked' | 'empty' | 'unreadable' }

export interface AttachDeps {
  readDocument: (kind: DocumentKind, data: string, maxChars: number) => Promise<DocumentOutcome>
  /** The file's bytes as base64. */
  encode: (file: Blob) => Promise<string>
}

export const MAX_FILE_CHARS = 200_000
export const MAX_TOTAL_CHARS = 500_000
export const MAX_FILES = 10
export const MAX_DOCUMENT_BYTES = 20 * 1024 * 1024
export const DOCUMENT_WAIT_MS = 75_000
const MAX_NAME = 120
/** UTF-8 spends at most four bytes a character. */
const MAX_CHAR_BYTES = 4

const DOCUMENT_KINDS: ReadonlyMap<string, DocumentKind> = new Map([
  ['pdf', 'pdf'],
  ['docx', 'docx'],
  ['doc', 'doc'],
  ['rtf', 'rtf'],
  ['odt', 'odt'],
])

const DOCUMENT_REFUSALS: Record<Exclude<DocumentOutcome['status'], 'read'>, string> = {
  locked: 'is password-protected',
  empty: 'has no text to read; it may be a scan',
  unreadable: 'could not be read',
}

const UNSAFE_NAME = /["<>\u0000-\u001f\u007f\u200b-\u200f\u2028\u2029\u202a-\u202e\u2066-\u2069]/g

const tidyName = (name: string) => name.replace(UNSAFE_NAME, '').trim().slice(0, MAX_NAME) || 'file'

const totalOf = (taken: readonly Attachment[]) => taken.reduce((sum, f) => sum + f.text.length, 0)

export function documentKind(name: string): DocumentKind | null {
  const extension = /\.([a-z0-9]+)$/i.exec(name)?.[1]?.toLowerCase()
  return (extension && DOCUMENT_KINDS.get(extension)) || null
}

/** The most text one more file may bring. */
export function roomFor(taken: readonly Attachment[]): number {
  return Math.max(0, Math.min(MAX_FILE_CHARS, MAX_TOTAL_CHARS - totalOf(taken)))
}

/** The start of a text that fits `room`, and what share of the file's
 *  `size` bytes that is when it was cut. */
export function fitText(text: string, room: number, size: number): { text: string; partial?: Partial } {
  if (text.length <= room) return { text }
  const kept = text.slice(0, room).replace(/[\uD800-\uDBFF]$/, '')
  return { text: kept, partial: { read: new TextEncoder().encode(kept).length, total: size, pages: false } }
}

const shareOf = (p: Partial) =>
  Math.min(99, Math.max(1, p.total > 0 ? Math.floor((p.read * 100) / p.total) : 0))

export function partialNote(p: Partial): string {
  return p.pages ? `the first ${p.read} of ${p.total} pages` : `the first ${shareOf(p)}%`
}

export function partialTag(p: Partial): string {
  return p.pages ? `${p.read}/${p.total} pages` : `${shareOf(p)}%`
}

function within<T>(work: Promise<T>, ms: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined
  const late = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error('The file took too long to read.')), ms)
  })
  return Promise.race([work, late]).finally(() => clearTimeout(timer))
}

function refusal(clean: string, taken: readonly Attachment[]): string | null {
  if (taken.length >= MAX_FILES) return `${clean}: at most ${MAX_FILES} files at once`
  if (taken.some((f) => f.name === clean)) return `${clean} is already attached`
  if (roomFor(taken) === 0) return `${clean}: too much attached text in one request`
  return null
}

export function acceptFile(name: string, text: string, taken: readonly Attachment[], partial?: Partial): Accepted {
  const clean = tidyName(name)
  const refused = refusal(clean, taken)
  if (refused) return { ok: false, reason: refused }
  if (text.includes('\u0000')) return { ok: false, reason: `${clean} is not a text file` }
  if (text.trim() === '') return { ok: false, reason: `${clean} is empty` }
  if (text.length > MAX_FILE_CHARS) return { ok: false, reason: `${clean} is too large` }
  if (text.length > roomFor(taken)) return { ok: false, reason: `${clean}: too much attached text in one request` }
  return { ok: true, file: partial ? { name: clean, text, partial } : { name: clean, text } }
}

type Read = { text: string; partial?: Partial } | { refused: string }

async function readFile(file: File, room: number, deps: AttachDeps): Promise<Read> {
  const kind = documentKind(file.name)
  if (kind === null) {
    const most = room * MAX_CHAR_BYTES + MAX_CHAR_BYTES
    const text = await (file.size > most ? file.slice(0, most) : file).text()
    return fitText(text, room, file.size)
  }
  if (file.size > MAX_DOCUMENT_BYTES) return { refused: `is over ${MAX_DOCUMENT_BYTES / 1024 / 1024} MB` }
  const outcome = await within(deps.readDocument(kind, await deps.encode(file), room), DOCUMENT_WAIT_MS)
  if (outcome.status !== 'read') return { refused: DOCUMENT_REFUSALS[outcome.status] }
  return { text: outcome.text, partial: outcome.partial ?? undefined }
}

/** Reads picked files one at a time, keeping what fits beside `taken`; notes
 *  hold the refusals and the files kept only in part. */
export async function attachFiles(
  files: readonly File[],
  taken: readonly Attachment[],
  deps: AttachDeps,
): Promise<{ added: Attachment[]; notes: string[] }> {
  const kept = [...taken]
  const notes: string[] = []
  for (const file of files) {
    const clean = tidyName(file.name)
    const refused = refusal(clean, kept)
    if (refused) {
      notes.push(refused)
      continue
    }
    const read = await readFile(file, roomFor(kept), deps).catch((): Read => ({ refused: DOCUMENT_REFUSALS.unreadable }))
    if ('refused' in read) {
      notes.push(`${clean} ${read.refused}`)
      continue
    }
    const result = acceptFile(file.name, read.text, kept, read.partial)
    if (!result.ok) {
      notes.push(result.reason)
      continue
    }
    kept.push(result.file)
    if (result.file.partial) notes.push(`${clean}: only ${partialNote(result.file.partial)} fit`)
  }
  return { added: kept.slice(taken.length), notes }
}

export function attachmentBlock(files: readonly Attachment[]): string {
  return files
    .map((f) => {
      const partial = f.partial ? ` partial="${partialNote(f.partial)}"` : ''
      return `<attachment name="${tidyName(f.name)}"${partial}>\n${f.text.replace(/<(\/?attachment)/gi, '&lt;$1')}\n</attachment>`
    })
    .join('\n')
}
