/**
 * Where the canvas agent's cards go: free space near where they were asked
 * for, groups kept in shape at the cards' real sizes, and room made below a
 * card that grows. Every placement keeps cards apart by `GAP`.
 */

import { FRAME_PAD, MAX_FRAME_SIZE, type Box, type CanvasFrame } from '@/features/canvas/lib/canvas'

export const GAP = 40
const STEP = 40
const MAX_RINGS = 30

export interface Budget {
  probes: number
}

type Placed = Box & { id: string }

/** Whether two boxes come within `gap` of each other. */
const near = (a: Box, b: Box, gap: number) =>
  a.x < b.x + b.w + gap && b.x < a.x + a.w + gap && a.y < b.y + b.h + gap && b.y < a.y + a.h + gap

const shareColumn = (a: Box, b: Box, gap: number) => a.x < b.x + b.w + gap && b.x < a.x + a.w + gap

const bounds = (boxes: readonly Box[]): Box => {
  const left = Math.min(...boxes.map((b) => b.x))
  const top = Math.min(...boxes.map((b) => b.y))
  const right = Math.max(...boxes.map((b) => b.x + b.w))
  const bottom = Math.max(...boxes.map((b) => b.y + b.h))
  return { x: left, y: top, w: right - left, h: bottom - top }
}

/** Below every card in the area's column: always clear. */
function underColumn(area: Box, taken: readonly Box[]): number {
  const floor = taken.filter((t) => shareColumn(area, t, GAP)).reduce((low, t) => Math.max(low, t.y + t.h), -Infinity)
  return floor === -Infinity ? area.y : Math.max(area.y, floor + GAP)
}

/** Offsets in steps, nearest first; ties go right, then down. */
const OFFSETS: readonly (readonly [number, number])[] = Array.from({ length: (2 * MAX_RINGS + 1) ** 2 }, (_, i) => {
  const dx = (i % (2 * MAX_RINGS + 1)) - MAX_RINGS
  const dy = Math.floor(i / (2 * MAX_RINGS + 1)) - MAX_RINGS
  return [dx, dy] as const
}).sort(
  ([ax, ay], [bx, by]) =>
    Math.hypot(ax, ay) - Math.hypot(bx, by) || Number(ax < 0) - Number(bx < 0) || Number(ay < 0) - Number(by < 0),
)

/** How far `boxes` must move together to clear `taken`: none when already
 *  clear, else the nearest clear offset, else straight down below their
 *  column. */
export function clearShift(boxes: readonly Box[], taken: readonly Box[], budget: Budget): { dx: number; dy: number } {
  if (boxes.length === 0) return { dx: 0, dy: 0 }
  const area = bounds(boxes)
  const nearby = taken.filter((t) => near(area, t, MAX_RINGS * STEP + GAP))
  const fits = (dx: number, dy: number) =>
    boxes.every((b) => nearby.every((t) => !near({ ...b, x: b.x + dx, y: b.y + dy }, t, GAP)))
  for (const [x, y] of OFFSETS) {
    budget.probes -= nearby.length * boxes.length + 1
    if (budget.probes < 0) break
    if (fits(x * STEP, y * STEP)) return { dx: x * STEP + 0, dy: y * STEP + 0 }
  }
  return { dx: 0, dy: underColumn(area, taken) - area.y }
}

/** The clear spot nearest `box`. */
export function freeSpot(box: Box, taken: readonly Box[], budget: Budget): { x: number; y: number } {
  const { dx, dy } = clearShift([box], taken, budget)
  return { x: box.x + dx, y: box.y + dy }
}

/** A group's boxes at their real sizes, spread only as far as needed: cards
 *  in a row (tops within `gap`) move right off each other, and each row moves
 *  down, as one, below the earlier cards in its columns. */
export function arrange(boxes: readonly Box[], gap: number = GAP): Box[] {
  const order = boxes.map((_, i) => i).sort((a, b) => boxes[a].y - boxes[b].y || boxes[a].x - boxes[b].x)
  const rows: number[][] = []
  for (const i of order) {
    const row = rows[rows.length - 1]
    if (row && boxes[i].y - boxes[row[0]].y < gap) row.push(i)
    else rows.push([i])
  }
  const out = boxes.map((b) => ({ ...b }))
  const done: Box[] = []
  for (const row of rows) {
    let right = -Infinity
    for (const i of [...row].sort((a, b) => boxes[a].x - boxes[b].x)) {
      out[i].x = Math.max(boxes[i].x, right + gap)
      right = out[i].x + out[i].w
    }
    const drop = row.reduce((most, i) => {
      const floor = done.filter((d) => shareColumn(out[i], d, gap)).reduce((low, d) => Math.max(low, d.y + d.h), -Infinity)
      return Math.max(most, floor + gap - boxes[i].y)
    }, 0)
    for (const i of row) {
      out[i].y = boxes[i].y + drop
      done.push(out[i])
    }
  }
  return out
}

/** Where the cards a grown box now covers go: each moves straight down past
 *  whatever is in its way, and the cards it then covers follow. Cards above
 *  the box, or still clear of it, stay. */
export function makeRoom(grown: Box, others: readonly Placed[], gap: number = GAP): Map<string, Box> {
  const moves = new Map<string, Box>()
  const pushers: Box[] = [grown]
  const still: Box[] = []
  const below = [...others].filter((o) => o.y >= grown.y).sort((a, b) => a.y - b.y || a.x - b.x)
  for (const other of below) {
    if (!pushers.some((p) => near(other, p, 0))) {
      still.push(other)
      continue
    }
    let box: Box = { x: other.x, y: other.y, w: other.w, h: other.h }
    for (let guard = 0; guard <= others.length; guard++) {
      const hit = pushers.find((p) => near(box, p, gap)) ?? still.find((p) => near(box, p, gap))
      if (!hit) break
      box = { ...box, y: hit.y + hit.h + gap }
    }
    moves.set(other.id, box)
    pushers.push(box)
  }
  return moves
}

const centreIn = (b: Box, f: Box) =>
  b.x + b.w / 2 >= f.x && b.x + b.w / 2 <= f.x + f.w && b.y + b.h / 2 >= f.y && b.y + b.h / 2 <= f.y + f.h

/** Frames grown to keep holding the cards they held before `after` moved or
 *  resized them; frames left alone stay the same objects. */
export function fitFrames(
  frames: readonly CanvasFrame[],
  before: readonly Placed[],
  after: ReadonlyMap<string, Box>,
): readonly CanvasFrame[] {
  let changed = false
  const fitted = frames.map((frame) => {
    let right = frame.x + frame.w
    let bottom = frame.y + frame.h
    for (const card of before) {
      const now = after.get(card.id)
      if (!now || !centreIn(card, frame)) continue
      right = Math.max(right, now.x + now.w + FRAME_PAD)
      bottom = Math.max(bottom, now.y + now.h + FRAME_PAD)
    }
    const w = Math.min(MAX_FRAME_SIZE, right - frame.x)
    const h = Math.min(MAX_FRAME_SIZE, bottom - frame.y)
    if (w === frame.w && h === frame.h) return frame
    changed = true
    return { ...frame, w, h }
  })
  return changed ? fitted : frames
}
