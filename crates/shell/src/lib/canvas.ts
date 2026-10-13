/** Rust stores the document opaquely, so reading one back trusts nothing. */

import { fuzzyMatch } from '@/lib/palette'

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

export function moveCards(
  doc: CanvasDoc,
  ids: ReadonlySet<string>,
  dx: number,
  dy: number,
): CanvasDoc {
  return {
    ...doc,
    cards: doc.cards.map((c) => (ids.has(c.id) ? { ...c, x: c.x + dx, y: c.y + dy } : c)),
  }
}

export function removeCards(doc: CanvasDoc, ids: ReadonlySet<string>): CanvasDoc {
  return { ...doc, cards: doc.cards.filter((c) => !ids.has(c.id)) }
}

export interface Box {
  x: number
  y: number
  w: number
  h: number
}

export function boxBetween(ax: number, ay: number, bx: number, by: number): Box {
  return { x: Math.min(ax, bx), y: Math.min(ay, by), w: Math.abs(bx - ax), h: Math.abs(by - ay) }
}

export function cardsTouching(boxes: ReadonlyArray<Box & { id: string }>, box: Box): string[] {
  return boxes
    .filter((b) => b.x <= box.x + box.w && b.x + b.w >= box.x && b.y <= box.y + box.h && b.y + b.h >= box.y)
    .map((b) => b.id)
}

export const HISTORY_LIMIT = 100

export interface History {
  past: CanvasDoc[]
  future: CanvasDoc[]
}

export function emptyHistory(): History {
  return { past: [], future: [] }
}

export function record(history: History, before: CanvasDoc): History {
  return { past: [...history.past, before].slice(-HISTORY_LIMIT), future: [] }
}

export function undo(history: History, current: CanvasDoc): { history: History; doc: CanvasDoc } | null {
  const previous = history.past[history.past.length - 1]
  if (previous === undefined) return null
  return {
    history: { past: history.past.slice(0, -1), future: [current, ...history.future] },
    doc: { ...previous, view: current.view },
  }
}

export function redo(history: History, current: CanvasDoc): { history: History; doc: CanvasDoc } | null {
  const next = history.future[0]
  if (next === undefined) return null
  return {
    history: { past: [...history.past, current], future: history.future.slice(1) },
    doc: { ...next, view: current.view },
  }
}

export function withView(doc: CanvasDoc, view: CanvasView): CanvasDoc {
  return { ...doc, view }
}

const MAX_NAME_CHARS = 120

/** Matches Rust's `canvas_name`. */
export function tidyName(raw: string): string {
  const name = raw.split(/\s+/).filter(Boolean).join(' ')
  return Array.from(name).slice(0, MAX_NAME_CHARS).join('')
}

export function matchCanvases(
  canvases: readonly CanvasSummary[],
  query: string,
): CanvasSummary[] {
  const q = query.trim()
  if (q === '') return [...canvases].sort((a, b) => b.updatedAt - a.updatedAt)
  return canvases
    .map((canvas) => ({ canvas, score: fuzzyMatch(canvas.name, q).score }))
    .filter((m) => m.score > 0)
    .sort((a, b) => b.score - a.score || b.canvas.updatedAt - a.canvas.updatedAt)
    .map((m) => m.canvas)
}

export function newCard(x: number, y: number, id: string = crypto.randomUUID()): CanvasCard {
  return { id, x: x - CARD_WIDTH / 2, y: y - 20, w: CARD_WIDTH, text: '' }
}

export function toCanvas(view: CanvasView, sx: number, sy: number): { x: number; y: number } {
  return { x: (sx - view.x) / view.k, y: (sy - view.y) / view.k }
}

export function zoomAt(view: CanvasView, sx: number, sy: number, factor: number): CanvasView {
  const k = clamp(view.k * factor, MIN_ZOOM, MAX_ZOOM)
  return { x: sx - ((sx - view.x) * k) / view.k, y: sy - ((sy - view.y) * k) / view.k, k }
}
