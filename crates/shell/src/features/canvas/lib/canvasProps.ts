/**
 * The canvas props panel: what it shows for the selection, a picked frame or
 * a picked arrow, and the documents its fields make.
 */

import {
  cardsInFrame,
  frameAround,
  framesInside,
  isCardTint,
  MAX_CARD_HEIGHT,
  MAX_CARD_WIDTH,
  MAX_COORD,
  MIN_CARD_HEIGHT,
  MIN_CARD_WIDTH,
  moveFrames,
  renameFrame,
  resizeFrame,
  tidyName,
  type Box,
  type CanvasCard,
  type CanvasDoc,
  type EdgeHeads,
  type EdgeShape,
  type CardTint,
  withColor,
} from '@/features/canvas/lib/canvas'
import { titleOf } from '@/features/canvas/lib/canvasAgent'

export type Picked = { kind: 'edge' | 'frame'; id: string } | null
export type Field = 'x' | 'y' | 'w' | 'h'
export type Tint = CardTint
export type { EdgeHeads, EdgeShape } from '@/features/canvas/lib/canvas'
export { styleEdge as setEdge } from '@/features/canvas/lib/canvas'
type HeightOf = (card: CanvasCard) => number

export type PropsView =
  | {
      kind: 'cards'
      ids: string[]
      label: string
      title: string
      box: Box
      /** Shared by every card, or null when they differ. */
      w: number | null
      h: number | null
      /** Whether every card takes a height of its own (visuals do; notes follow their text). */
      sized: boolean
      /** Shared by every card, or null when they differ. */
      color: Tint | null
    }
  | { kind: 'frame'; id: string; title: string; box: Box; color: Tint }
  | { kind: 'edge'; id: string; from: string; to: string; shape: EdgeShape; heads: EdgeHeads; label: string }
  | null

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n))
const shared = (values: readonly number[]) => (values.every((v) => v === values[0]) ? values[0] : null)
const named = (card: CanvasCard | undefined) => (card ? titleOf(card) || 'Untitled' : 'a removed card')

const boxOf = (card: CanvasCard, heightOf: HeightOf): Box => ({ x: card.x, y: card.y, w: card.w, h: heightOf(card) })

export function propsView(
  doc: CanvasDoc,
  selected: ReadonlySet<string>,
  picked: Picked,
  heightOf: HeightOf,
): PropsView {
  if (picked?.kind === 'frame') {
    const frame = doc.frames.find((f) => f.id === picked.id)
    if (!frame) return null
    const box = { x: frame.x, y: frame.y, w: frame.w, h: frame.h }
    return { kind: 'frame', id: frame.id, title: frame.title, box, color: frame.color ?? 'none' }
  }
  if (picked?.kind === 'edge') {
    const edge = doc.edges.find((e) => e.id === picked.id)
    if (!edge) return null
    const card = (id: string) => doc.cards.find((c) => c.id === id)
    return {
      kind: 'edge',
      id: edge.id,
      from: named(card(edge.from)),
      to: named(card(edge.to)),
      shape: edge.shape ?? 'curved',
      heads: edge.heads ?? 'end',
      label: edge.label ?? '',
    }
  }
  const cards = doc.cards.filter((c) => selected.has(c.id))
  if (cards.length === 0) return null
  const boxes = cards.map((c) => boxOf(c, heightOf))
  const one = cards.length === 1 ? cards[0] : null
  return {
    kind: 'cards',
    ids: cards.map((c) => c.id),
    label: one ? (one.kind === 'html' ? 'Visual' : 'Note') : `${cards.length} cards`,
    title: one ? titleOf(one) : '',
    box: frameAround(boxes, 0),
    w: shared(boxes.map((b) => b.w)),
    h: shared(boxes.map((b) => b.h)),
    sized: cards.every((c) => c.kind === 'html'),
    color: sharedTint(cards.map((c) => c.color ?? 'none')),
  }
}

const sharedTint = (tints: readonly Tint[]) => (tints.every((t) => t === tints[0]) ? tints[0] : null)


export function setCardsColor(doc: CanvasDoc, ids: readonly string[], tint: string): CanvasDoc {
  if (!isCardTint(tint)) return doc
  const picked = new Set(ids)
  const takes = (c: CanvasCard) => picked.has(c.id) && (c.color ?? 'none') !== tint
  if (!doc.cards.some(takes)) return doc
  return { ...doc, cards: doc.cards.map((c) => (takes(c) ? withColor(c, tint) : c)) }
}

export function setFrameColor(doc: CanvasDoc, id: string, tint: string): CanvasDoc {
  const frame = doc.frames.find((f) => f.id === id)
  if (!frame || !isCardTint(tint) || (frame.color ?? 'none') === tint) return doc
  return { ...doc, frames: doc.frames.map((f) => (f.id === id ? withColor(f, tint) : f)) }
}

/** Sets a field across cards: X and Y move them together so their bounds
 *  start there, W sets every width, H sizes the visuals among them. */
export function setCards(doc: CanvasDoc, ids: readonly string[], field: Field, value: number, heightOf: HeightOf): CanvasDoc {
  if (!Number.isFinite(value)) return doc
  const picked = new Set(ids)
  const cards = doc.cards.filter((c) => picked.has(c.id))
  if (cards.length === 0) return doc
  if (field === 'x' || field === 'y') {
    const box = frameAround(cards.map((c) => boxOf(c, heightOf)), 0)
    const delta = clamp(value, -MAX_COORD, MAX_COORD) - box[field]
    if (delta === 0) return doc
    return {
      ...doc,
      cards: doc.cards.map((c) =>
        picked.has(c.id) ? { ...c, [field]: clamp(c[field] + delta, -MAX_COORD, MAX_COORD) } : c,
      ),
    }
  }
  const size =
    field === 'w' ? clamp(value, MIN_CARD_WIDTH, MAX_CARD_WIDTH) : clamp(value, MIN_CARD_HEIGHT, MAX_CARD_HEIGHT)
  const takes = (c: CanvasCard) => picked.has(c.id) && (field === 'w' || c.kind === 'html') && c[field] !== size
  if (!doc.cards.some(takes)) return doc
  return { ...doc, cards: doc.cards.map((c) => (takes(c) ? { ...c, [field]: size } : c)) }
}

/** Sets a frame's field: X and Y move it with the cards and frames it holds,
 *  W and H resize it. */
export function setFrame(doc: CanvasDoc, id: string, field: Field, value: number, heightOf: HeightOf): CanvasDoc {
  const frame = doc.frames.find((f) => f.id === id)
  if (!frame || !Number.isFinite(value)) return doc
  if (field === 'w' || field === 'h') {
    return resizeFrame(doc, id, field === 'w' ? value : frame.w, field === 'h' ? value : frame.h)
  }
  const delta = clamp(value, -MAX_COORD, MAX_COORD) - frame[field]
  const held = new Set([frame.id, ...framesInside(doc.frames, frame)])
  const boxes = doc.cards.map((c) => ({ id: c.id, ...boxOf(c, heightOf) }))
  const cards = new Set(
    doc.frames.filter((f) => held.has(f.id)).flatMap((f) => cardsInFrame(boxes, f)),
  )
  return moveFrames(doc, held, cards, field === 'x' ? delta : 0, field === 'y' ? delta : 0)
}

export function setFrameTitle(doc: CanvasDoc, id: string, title: string): CanvasDoc {
  const tidy = tidyName(title)
  const frame = doc.frames.find((f) => f.id === id)
  return frame && tidy !== '' && tidy !== frame.title ? renameFrame(doc, id, tidy) : doc
}

/** A typed number, rounded; null for anything that is not one. */
export function readNumber(text: string): number | null {
  const trimmed = text.trim()
  if (!/^[-+]?(\d+\.?\d*|\.\d+)(e[-+]?\d+)?$/i.test(trimmed)) return null
  const value = Number(trimmed)
  return Number.isFinite(value) ? Math.round(value) : null
}
