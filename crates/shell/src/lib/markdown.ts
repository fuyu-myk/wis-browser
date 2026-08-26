/**
 * Pure markdown parser for the assistant's answers: text in, a typed block
 * tree out. No React and no HTML strings live here; the renderer maps the
 * tree to escaped elements, so untrusted model output can never smuggle
 * markup onto the privileged chrome surface.
 *
 * Deliberately a subset. Anything unrecognised falls through as plain text.
 */

export type Span =
  | { kind: 'text'; text: string }
  | { kind: 'strong'; text: string }
  | { kind: 'em'; text: string }
  | { kind: 'strike'; text: string }
  | { kind: 'code'; text: string }
  | { kind: 'link'; text: string; href: string }

export interface ListItem {
  depth: number
  spans: Span[]
}

export type Block =
  | { kind: 'paragraph'; spans: Span[] }
  | { kind: 'heading'; level: 1 | 2 | 3; spans: Span[] }
  | { kind: 'code'; lang: string; text: string }
  | { kind: 'list'; ordered: boolean; items: ListItem[] }
  | { kind: 'quote'; lines: Span[][] }
  | { kind: 'table'; header: Span[][]; rows: Span[][][] }
  | { kind: 'rule' }

const HEADING = /^(#{1,6})\s+(.*)$/
const FENCE = /^```([A-Za-z0-9+#._-]*)\s*$/
const LIST_ITEM = /^(\s*)([-*+]|\d{1,3}[.)])\s+(.*)$/
const QUOTE = /^>\s?(.*)$/
const RULE = /^ {0,3}(?:-{3,}|\*{3,}|_{3,})\s*$/
const TABLE_ROW = /^\s*\|(.+)\|\s*$/
const TABLE_SEPARATOR = /^\s*\|?(?:\s*:?-+:?\s*\|)+\s*:?-+:?\s*\|?\s*$/

/** Split a `| a | b |` row into trimmed cell strings, honouring `\|`. */
function tableCells(inner: string): string[] {
  const cells: string[] = []
  let current = ''
  for (let i = 0; i < inner.length; i += 1) {
    const ch = inner[i]
    if (ch === '\\' && inner[i + 1] === '|') {
      current += '|'
      i += 1
    } else if (ch === '|') {
      cells.push(current.trim())
      current = ''
    } else {
      current += ch
    }
  }
  cells.push(current.trim())
  return cells
}

/**
 * Inline parsing: code spans first so nothing inside backticks is styled,
 * then links, then the emphasis pairs. Not recursive; emphasis inside link
 * text stays literal.
 */
export function parseSpans(text: string): Span[] {
  const spans: Span[] = []
  let rest = text

  // Underscore emphasis requires word boundaries so snake_case identifiers
  // in prose stay literal; CommonMark draws the same line.
  const INLINE =
    /(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)|\[([^\]\n]+)\]\(([^\s)]+)\)|\*\*([^*\n]+)\*\*|(?<![A-Za-z0-9])__([^_\n]+)__(?![A-Za-z0-9])|\*([^*\n]+)\*|(?<![A-Za-z0-9])_([^_\n]+)_(?![A-Za-z0-9])|~~([^~\n]+)~~/

  while (rest !== '') {
    const match = INLINE.exec(rest)
    if (!match) {
      spans.push({ kind: 'text', text: rest })
      break
    }
    if (match.index > 0) {
      spans.push({ kind: 'text', text: rest.slice(0, match.index) })
    }
    if (match[2] !== undefined) {
      spans.push({ kind: 'code', text: match[2].trim() })
    } else if (match[3] !== undefined && match[4] !== undefined) {
      spans.push({ kind: 'link', text: match[3], href: match[4] })
    } else if (match[5] !== undefined || match[6] !== undefined) {
      spans.push({ kind: 'strong', text: match[5] ?? match[6] ?? '' })
    } else if (match[7] !== undefined || match[8] !== undefined) {
      spans.push({ kind: 'em', text: match[7] ?? match[8] ?? '' })
    } else {
      spans.push({ kind: 'strike', text: match[9] ?? '' })
    }
    rest = rest.slice(match.index + match[0].length)
  }

  return spans
}

/** Parse `text` into blocks. Safe to re-run on every streamed frame. */
export function parseMarkdown(text: string): Block[] {
  const lines = text.split('\n')
  const blocks: Block[] = []
  let paragraph: string[] = []

  const flushParagraph = () => {
    if (paragraph.length === 0) return
    blocks.push({ kind: 'paragraph', spans: parseSpans(paragraph.join('\n')) })
    paragraph = []
  }

  let i = 0
  while (i < lines.length) {
    const line = lines[i]

    const fence = FENCE.exec(line)
    if (fence) {
      flushParagraph()
      const body: string[] = []
      i += 1
      while (i < lines.length && !/^```\s*$/.test(lines[i])) {
        body.push(lines[i])
        i += 1
      }
      i += 1
      blocks.push({ kind: 'code', lang: fence[1] ?? '', text: body.join('\n') })
      continue
    }

    if (line.trim() === '') {
      flushParagraph()
      i += 1
      continue
    }

    if (RULE.test(line)) {
      flushParagraph()
      blocks.push({ kind: 'rule' })
      i += 1
      continue
    }

    const heading = HEADING.exec(line)
    if (heading) {
      flushParagraph()
      const level = Math.min(heading[1].length, 3) as 1 | 2 | 3
      blocks.push({ kind: 'heading', level, spans: parseSpans(heading[2]) })
      i += 1
      continue
    }

    if (QUOTE.test(line)) {
      flushParagraph()
      const quoteLines: Span[][] = []
      while (i < lines.length) {
        const inner = QUOTE.exec(lines[i])
        if (!inner) break
        quoteLines.push(parseSpans(inner[1]))
        i += 1
      }
      blocks.push({ kind: 'quote', lines: quoteLines })
      continue
    }

    const item = LIST_ITEM.exec(line)
    if (item) {
      flushParagraph()
      const ordered = /\d/.test(item[2])
      const items: ListItem[] = []
      while (i < lines.length) {
        const next = LIST_ITEM.exec(lines[i])
        if (!next) break
        const depth = Math.min(Math.floor(next[1].length / 2), 2)
        items.push({ depth, spans: parseSpans(next[3]) })
        i += 1
      }
      blocks.push({ kind: 'list', ordered, items })
      continue
    }

    const row = TABLE_ROW.exec(line)
    if (row && i + 1 < lines.length && TABLE_SEPARATOR.test(lines[i + 1])) {
      flushParagraph()
      const header = tableCells(row[1]).map(parseSpans)
      i += 2
      const rows: Span[][][] = []
      while (i < lines.length) {
        const bodyRow = TABLE_ROW.exec(lines[i])
        if (!bodyRow) break
        rows.push(tableCells(bodyRow[1]).map(parseSpans))
        i += 1
      }
      blocks.push({ kind: 'table', header, rows })
      continue
    }

    paragraph.push(line)
    i += 1
  }

  flushParagraph()
  return blocks
}
