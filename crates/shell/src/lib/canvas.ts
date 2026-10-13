/**
 * The document behind a canvas: its cards and where the view rests. Stored as JSON by
 * Rust, which only bounds it, so reading one back trusts nothing.
 */

export interface CanvasView {
  x: number
  y: number
  k: number
}

export interface CanvasCard {
  id: string
  x: number
  y: number
  w: number
  text: string
}

export interface CanvasDoc {
  version: 1
  view: CanvasView
  cards: CanvasCard[]
}

export interface CanvasSummary {
  id: string
  name: string
  updatedAt: number
}

export const MIN_ZOOM = 0.1
export const MAX_ZOOM = 4
export const CARD_WIDTH = 280
export const MIN_CARD_WIDTH = 120
export const MAX_CARD_WIDTH = 1200
const MAX_CARD_TEXT = 20_000
const MAX_CARDS = 5000
const MAX_COORD = 10_000_000
const MAX_ID = 64

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n))
const finite = (n: unknown): n is number => typeof n === 'number' && Number.isFinite(n)
const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v)

export function emptyDoc(): CanvasDoc {
  return { version: 1, view: { x: 0, y: 0, k: 1 }, cards: [] }
}

function readView(raw: unknown): CanvasView {
  if (!isRecord(raw) || !finite(raw.x) || !finite(raw.y) || !finite(raw.k)) return emptyDoc().view
  return {
    x: clamp(raw.x, -MAX_COORD, MAX_COORD),
    y: clamp(raw.y, -MAX_COORD, MAX_COORD),
    k: clamp(raw.k, MIN_ZOOM, MAX_ZOOM),
  }
}

function readCard(raw: unknown): CanvasCard | null {
  if (!isRecord(raw)) return null
  const { id, x, y, w, text } = raw
  if (typeof id !== 'string' || id === '' || id.length > MAX_ID) return null
  if (!finite(x) || !finite(y) || typeof text !== 'string') return null
  return {
    id,
    x: clamp(x, -MAX_COORD, MAX_COORD),
    y: clamp(y, -MAX_COORD, MAX_COORD),
    w: clamp(finite(w) ? w : CARD_WIDTH, MIN_CARD_WIDTH, MAX_CARD_WIDTH),
    text: text.slice(0, MAX_CARD_TEXT),
  }
}

/** A stored document, or a blank canvas when it cannot be read. */
export function parseDoc(text: string): CanvasDoc {
  let raw: unknown
  try {
    raw = JSON.parse(text)
  } catch {
    return emptyDoc()
  }
  if (!isRecord(raw) || raw.version !== 1) return emptyDoc()
  const seen = new Set<string>()
  const cards: CanvasCard[] = []
  for (const entry of Array.isArray(raw.cards) ? raw.cards : []) {
    const card = readCard(entry)
    if (card === null || seen.has(card.id)) continue
    seen.add(card.id)
    cards.push(card)
    if (cards.length === MAX_CARDS) break
  }
  return { version: 1, view: readView(raw.view), cards }
}

export function serializeDoc(doc: CanvasDoc): string {
  return JSON.stringify(doc)
}

export function addCard(doc: CanvasDoc, card: CanvasCard): CanvasDoc {
  return { ...doc, cards: [...doc.cards, card] }
}

export function updateCard(
  doc: CanvasDoc,
  id: string,
  patch: Partial<Omit<CanvasCard, 'id'>>,
): CanvasDoc {
  if (!doc.cards.some((c) => c.id === id)) return doc
  return { ...doc, cards: doc.cards.map((c) => (c.id === id ? { ...c, ...patch } : c)) }
}

export function removeCard(doc: CanvasDoc, id: string): CanvasDoc {
  return { ...doc, cards: doc.cards.filter((c) => c.id !== id) }
}

export function withView(doc: CanvasDoc, view: CanvasView): CanvasDoc {
  return { ...doc, view }
}

/** A blank card centred on a canvas point. */
export function newCard(x: number, y: number, id: string = crypto.randomUUID()): CanvasCard {
  return { id, x: x - CARD_WIDTH / 2, y: y - 20, w: CARD_WIDTH, text: '' }
}

/** The canvas point under a screen point. */
export function toCanvas(view: CanvasView, sx: number, sy: number): { x: number; y: number } {
  return { x: (sx - view.x) / view.k, y: (sy - view.y) / view.k }
}

/** Zoom by `factor` about a screen point, keeping that point still. */
export function zoomAt(view: CanvasView, sx: number, sy: number, factor: number): CanvasView {
  const k = clamp(view.k * factor, MIN_ZOOM, MAX_ZOOM)
  return { x: sx - ((sx - view.x) * k) / view.k, y: sy - ((sy - view.y) * k) / view.k, k }
}
