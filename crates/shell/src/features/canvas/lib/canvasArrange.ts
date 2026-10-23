/**
 * Arranging selected cards from the props panel: aligning edges or centres,
 * evening the gaps, and tidying into a grid. Cards are never left on top of
 * each other.
 */

import type { Box, CanvasCard, CanvasDoc } from '@/features/canvas/lib/canvas'
import { clearShift, GAP } from '@/features/canvas/lib/canvasLayout'

export type Edge = 'left' | 'centre' | 'right' | 'top' | 'middle' | 'bottom'
export type Axis = 'across' | 'down'
type HeightOf = (card: CanvasCard) => number
type Placed = Box & { id: string }

const MAX_PROBES = 400_000

const covers = (a: Box, b: Box) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h

function boxesOf(doc: CanvasDoc, ids: readonly string[], heightOf: HeightOf): Placed[] {
  const picked = new Set(ids)
  return doc.cards.filter((c) => picked.has(c.id)).map((c) => ({ id: c.id, x: c.x, y: c.y, w: c.w, h: heightOf(c) }))
}

/** The document with cards at `moved`; the same document when none moved. */
function place(doc: CanvasDoc, moved: readonly Placed[]): CanvasDoc {
  const to = new Map(moved.map((b) => [b.id, b]))
  const shifts = (c: CanvasCard) => {
    const b = to.get(c.id)
    return b !== undefined && (Math.round(b.x) !== c.x || Math.round(b.y) !== c.y)
  }
  if (!doc.cards.some(shifts)) return doc
  return {
    ...doc,
    cards: doc.cards.map((c) => {
      const b = to.get(c.id)
      return b && shifts(c) ? { ...c, x: Math.round(b.x), y: Math.round(b.y) } : c
    }),
  }
}

/** Moves cards that now overlap apart along `axis`, keeping the order they
 *  had before (`before`), each just past the card in its way. */
function separate(boxes: readonly Placed[], before: readonly Placed[], axis: Axis): Placed[] {
  const order = new Map(before.map((b) => [b.id, b]))
  const key = (b: Placed) => {
    const was = order.get(b.id) ?? b
    return axis === 'down' ? [was.y, was.x] : [was.x, was.y]
  }
  const sorted = [...boxes].sort((a, b) => {
    const [a1, a2] = key(a)
    const [b1, b2] = key(b)
    return a1 - b1 || a2 - b2
  })
  const placed: Placed[] = []
  for (const box of sorted) {
    let next = box
    for (let hit = placed.find((p) => covers(next, p)); hit; hit = placed.find((p) => covers(next, p))) {
      next = axis === 'down' ? { ...next, y: hit.y + hit.h + GAP } : { ...next, x: hit.x + hit.w + GAP }
    }
    placed.push(next)
  }
  return placed
}

export function alignCards(doc: CanvasDoc, ids: readonly string[], edge: Edge, heightOf: HeightOf): CanvasDoc {
  const boxes = boxesOf(doc, ids, heightOf)
  if (boxes.length < 2) return doc
  const left = Math.min(...boxes.map((b) => b.x))
  const right = Math.max(...boxes.map((b) => b.x + b.w))
  const top = Math.min(...boxes.map((b) => b.y))
  const bottom = Math.max(...boxes.map((b) => b.y + b.h))
  const lined = boxes.map((b) => {
    switch (edge) {
      case 'left':
        return { ...b, x: left }
      case 'centre':
        return { ...b, x: (left + right) / 2 - b.w / 2 }
      case 'right':
        return { ...b, x: right - b.w }
      case 'top':
        return { ...b, y: top }
      case 'middle':
        return { ...b, y: (top + bottom) / 2 - b.h / 2 }
      case 'bottom':
        return { ...b, y: bottom - b.h }
    }
  })
  const spread = edge === 'left' || edge === 'centre' || edge === 'right' ? 'down' : 'across'
  return place(doc, separate(lined, boxes, spread))
}

/** Evens the gaps between cards along `axis`, the outer two staying put; too
 *  little room spreads them `GAP` apart instead. */
export function distributeCards(doc: CanvasDoc, ids: readonly string[], axis: Axis, heightOf: HeightOf): CanvasDoc {
  const boxes = boxesOf(doc, ids, heightOf)
  if (boxes.length < 3) return doc
  const start = (b: Box) => (axis === 'across' ? b.x : b.y)
  const size = (b: Box) => (axis === 'across' ? b.w : b.h)
  const sorted = [...boxes].sort((a, b) => start(a) - start(b))
  const first = sorted[0]
  const last = sorted[sorted.length - 1]
  const span = Math.max(...sorted.map((b) => start(b) + size(b))) - start(first)
  const even = (span - sorted.reduce((sum, b) => sum + size(b), 0)) / (sorted.length - 1)
  const gap = Math.max(GAP, even)
  let at = start(first)
  const spaced = sorted.map((b) => {
    const moved = b === last && gap === even ? b : axis === 'across' ? { ...b, x: at } : { ...b, y: at }
    at += size(b) + gap
    return moved
  })
  return place(doc, spaced)
}

/** Cards in reading order: rows by top edge (within `GAP`), left to right. */
function readingOrder(boxes: readonly Placed[]): Placed[] {
  const byTop = [...boxes].sort((a, b) => a.y - b.y || a.x - b.x)
  const rows: Placed[][] = []
  for (const box of byTop) {
    const row = rows[rows.length - 1]
    if (row && box.y - row[0].y < GAP) row.push(box)
    else rows.push([box])
  }
  return rows.flatMap((row) => row.sort((a, b) => a.x - b.x))
}

/** Lays the cards out in a near-square grid from the first card's place,
 *  columns as wide and rows as tall as their largest card, moved clear of
 *  the cards around as a whole. */
export function tidyCards(doc: CanvasDoc, ids: readonly string[], heightOf: HeightOf): CanvasDoc {
  const boxes = readingOrder(boxesOf(doc, ids, heightOf))
  if (boxes.length < 2) return doc
  const columns = Math.ceil(Math.sqrt(boxes.length))
  const widths = Array.from({ length: columns }, (_, col) =>
    Math.max(...boxes.filter((_, i) => i % columns === col).map((b) => b.w)),
  )
  const rows = Math.ceil(boxes.length / columns)
  const heights = Array.from({ length: rows }, (_, row) =>
    Math.max(...boxes.slice(row * columns, (row + 1) * columns).map((b) => b.h)),
  )
  const offset = (sizes: readonly number[], n: number) => sizes.slice(0, n).reduce((sum, s) => sum + s + GAP, 0)
  const grid = boxes.map((b, i) => ({
    ...b,
    x: boxes[0].x + offset(widths, i % columns),
    y: boxes[0].y + offset(heights, Math.floor(i / columns)),
  }))
  const picked = new Set(ids)
  const others = doc.cards.filter((c) => !picked.has(c.id)).map((c) => ({ x: c.x, y: c.y, w: c.w, h: heightOf(c) }))
  const { dx, dy } = clearShift(grid, others, { probes: MAX_PROBES })
  return place(
    doc,
    grid.map((b) => ({ ...b, x: b.x + dx, y: b.y + dy })),
  )
}
