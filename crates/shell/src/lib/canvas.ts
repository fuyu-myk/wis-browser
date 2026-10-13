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

export interface CanvasEdge {
  id: string
  from: string
  to: string
}

export interface CanvasDoc {
  version: 1
  view: CanvasView
  cards: CanvasCard[]
  edges: CanvasEdge[]
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
const MAX_EDGES = 10_000
const MAX_COORD = 10_000_000
const MAX_ID = 64

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n))
const finite = (n: unknown): n is number => typeof n === 'number' && Number.isFinite(n)
const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v)

export function emptyDoc(): CanvasDoc {
  return { version: 1, view: { x: 0, y: 0, k: 1 }, cards: [], edges: [] }
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
  return { version: 1, view: readView(raw.view), cards, edges: readEdges(raw.edges, seen) }
}

function readEdges(raw: unknown, cardIds: ReadonlySet<string>): CanvasEdge[] {
  const edges: CanvasEdge[] = []
  const pairs = new Set<string>()
  for (const entry of Array.isArray(raw) ? raw : []) {
    if (!isRecord(entry)) continue
    const { id, from, to } = entry
    if (typeof id !== 'string' || id === '' || id.length > MAX_ID) continue
    if (typeof from !== 'string' || typeof to !== 'string' || from === to) continue
    if (!cardIds.has(from) || !cardIds.has(to) || pairs.has(`${from}>${to}`)) continue
    pairs.add(`${from}>${to}`)
    edges.push({ id, from, to })
    if (edges.length === MAX_EDGES) break
  }
  return edges
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
  return removeCards(doc, new Set([id]))
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
  return {
    ...doc,
    cards: doc.cards.filter((c) => !ids.has(c.id)),
    edges: doc.edges.filter((e) => !ids.has(e.from) && !ids.has(e.to)),
  }
}

export function addEdge(
  doc: CanvasDoc,
  from: string,
  to: string,
  id: string = crypto.randomUUID(),
): CanvasDoc {
  const known = (cardId: string) => doc.cards.some((c) => c.id === cardId)
  const exists = doc.edges.some((e) => e.from === from && e.to === to)
  if (from === to || exists || !known(from) || !known(to)) return doc
  return { ...doc, edges: [...doc.edges, { id, from, to }] }
}

export function removeEdge(doc: CanvasDoc, id: string): CanvasDoc {
  return { ...doc, edges: doc.edges.filter((e) => e.id !== id) }
}

export const ARROW_LENGTH = 12

interface Point {
  x: number
  y: number
}

/**
 * A curve leaving `start` along `out` and arriving at `tip` from roughly
 * `into`, leaning toward `start` so the head turns with it. It stops one
 * head-length short, aimed so the head's tip lands on `tip`.
 */
export function curvedArrow(start: Point, out: Point, tip: Point, into: Point, reach: number): string {
  const c1 = { x: start.x + out.x * reach, y: start.y + out.y * reach }
  const c2 = {
    x: tip.x + into.x * reach * 0.6 + (start.x - tip.x) * 0.2,
    y: tip.y + into.y * reach * 0.6 + (start.y - tip.y) * 0.2,
  }
  let ux = tip.x - c2.x
  let uy = tip.y - c2.y
  if (ux === 0 && uy === 0) {
    ux = tip.x - start.x
    uy = tip.y - start.y
  }
  const length = Math.hypot(ux, uy) || 1
  const back = Math.min(ARROW_LENGTH, Math.hypot(tip.x - start.x, tip.y - start.y)) / length
  return `M${start.x},${start.y} C${c1.x},${c1.y} ${c2.x - ux * back},${c2.y - uy * back} ${tip.x - ux * back},${tip.y - uy * back}`
}

/** Between the middles of the sides of `a` and `b` that face each other. */
export function edgeGeometry(a: Box, b: Box): { x1: number; y1: number; x2: number; y2: number; d: string } {
  const dx = b.x + b.w / 2 - (a.x + a.w / 2)
  const dy = b.y + b.h / 2 - (a.y + a.h / 2)
  const across = Math.abs(dx) >= Math.abs(dy)
  const s = (across ? dx : dy) < 0 ? -1 : 1
  const start = across
    ? { x: s > 0 ? a.x + a.w : a.x, y: a.y + a.h / 2 }
    : { x: a.x + a.w / 2, y: s > 0 ? a.y + a.h : a.y }
  const tip = across
    ? { x: s > 0 ? b.x : b.x + b.w, y: b.y + b.h / 2 }
    : { x: b.x + b.w / 2, y: s > 0 ? b.y : b.y + b.h }
  const out = across ? { x: s, y: 0 } : { x: 0, y: s }
  const into = { x: -out.x, y: -out.y }
  const reach = Math.max(40, Math.abs(across ? tip.x - start.x : tip.y - start.y) / 2)
  return { x1: start.x, y1: start.y, x2: tip.x, y2: tip.y, d: curvedArrow(start, out, tip, into, reach) }
}

/** The link being drawn from a side's middle to the pointer. */
export function pointerArrow(start: Point, out: Point, tip: Point): string {
  const dist = Math.hypot(tip.x - start.x, tip.y - start.y)
  const into = dist === 0 ? { x: 0, y: 0 } : { x: (start.x - tip.x) / dist, y: (start.y - tip.y) / dist }
  return curvedArrow(start, out, tip, into, Math.max(40, dist / 2))
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
