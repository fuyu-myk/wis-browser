// Where the canvas agent's cards go (`features/canvas/lib/canvasLayout.ts`): free space, groups
// kept in shape at their real sizes, and room made when a card grows.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import * as layout from '../../src/features/canvas/lib/canvasLayout.ts'

const box = (x, y, w = 100, h = 100) => ({ x, y, w, h })
const touching = (a, b, gap = 0) =>
  a.x < b.x + b.w + gap && b.x < a.x + a.w + gap && a.y < b.y + b.h + gap && b.y < a.y + a.h + gap
const apart = (boxes, gap = layout.GAP) => {
  for (let i = 0; i < boxes.length; i++)
    for (let j = i + 1; j < boxes.length; j++)
      if (touching(boxes[i], boxes[j], gap)) return `${i} and ${j} touch`
  return null
}
const budget = () => ({ probes: 400_000 })

describe('free space', () => {
  test('a clear spot is kept, a taken one moves to the nearest clear spot', () => {
    assert.deepEqual(layout.freeSpot(box(0, 0), [box(500, 500)], budget()), { x: 0, y: 0 })
    const spot = layout.freeSpot(box(0, 0), [box(0, 0)], budget())
    assert.equal(apart([{ ...box(0, 0), ...spot }, box(0, 0)]), null)
    assert.ok(Math.hypot(spot.x, spot.y) <= 200, `${spot.x},${spot.y} is near`)
  })

  test('when the search runs out it goes below everything in its column, never onto a card', () => {
    const column = Array.from({ length: 50 }, (_, i) => box(-2000, i * 140 - 3500, 4000, 100))
    const spot = layout.freeSpot(box(0, 0), column, budget())
    assert.equal(apart([{ ...box(0, 0), ...spot }, ...column]), null)
    const spent = layout.freeSpot(box(0, 0), [box(0, 0)], { probes: 0 })
    assert.equal(apart([{ ...box(0, 0), ...spent }, box(0, 0)]), null)
  })
})

describe('a group placed by the agent', () => {
  test('keeps its rows and columns, spread to the real sizes', () => {
    // A 2 × 2 grid laid out for 100-tall cards; the first is really 300 tall.
    const asked = [box(0, 0, 200, 300), box(240, 0, 200, 100), box(0, 140, 200, 100), box(240, 140, 200, 100)]
    const placed = layout.arrange(asked)
    assert.equal(apart(placed), null)
    assert.equal(placed[0].y, placed[1].y, 'the first row stays a row')
    assert.equal(placed[2].y, placed[3].y, 'the second row stays a row')
    assert.equal(placed[0].x, placed[2].x, 'columns stay columns')
    assert.equal(placed[2].y, 300 + layout.GAP, 'just below the tall card')
  })

  test('a row too tight for its widths is spread to the right', () => {
    const placed = layout.arrange([box(0, 0, 480, 200), box(300, 0, 480, 200), box(600, 0, 480, 200)])
    assert.equal(apart(placed), null)
    assert.deepEqual(placed.map((b) => b.y), [0, 0, 0])
    assert.deepEqual(placed.map((b) => b.x), [0, 520, 1040])
  })

  test('room already between cards is kept, and a column beside a tall card is not pushed', () => {
    const asked = [box(0, 0, 200, 600), box(400, 0), box(400, 300)]
    assert.deepEqual(layout.arrange(asked), asked)
  })

  test('clears the cards already there as a whole, or stays where it is when clear', () => {
    const group = [box(0, 0), box(140, 0)]
    assert.deepEqual(layout.clearShift(group, [box(0, 400)], budget()), { dx: 0, dy: 0 })
    const around = [box(-200, 0), box(200, 0)]
    assert.deepEqual(layout.clearShift(around, [box(0, 0)], budget()), { dx: 0, dy: 0 }, 'cards around a card are fine')
    const shift = layout.clearShift(group, [box(100, 50)], budget())
    const moved = group.map((b) => ({ ...b, x: b.x + shift.dx, y: b.y + shift.dy }))
    assert.equal(apart([...moved, box(100, 50)]), null)
    assert.equal(moved[1].x - moved[0].x, 140, 'the group keeps its shape')
  })
})

describe('a card that grows', () => {
  test('pushes the cards it now covers below it, and those they then cover', () => {
    const grown = box(0, 0, 280, 500)
    const others = [
      { id: 'under', ...box(0, 200, 280, 100) },
      { id: 'next', ...box(0, 340, 280, 100) },
      { id: 'beside', ...box(400, 200, 280, 100) },
      { id: 'above', ...box(0, -300, 280, 100) },
    ]
    const moves = layout.makeRoom(grown, others)
    assert.deepEqual([...moves.keys()], ['under', 'next'])
    assert.equal(moves.get('under').y, 500 + layout.GAP)
    assert.equal(moves.get('next').y, 500 + layout.GAP + 100 + layout.GAP)
    assert.equal(moves.get('under').x, 0, 'cards keep their column')
  })

  test('cards already clear of it stay put, however tight', () => {
    const moves = layout.makeRoom(box(0, 0, 280, 100), [{ id: 'close', ...box(0, 110, 280, 100) }])
    assert.equal(moves.size, 0)
  })

  test('frames that held a card grow to keep holding it', () => {
    const frames = [
      { id: 'f', x: -32, y: -32, w: 344, h: 264, title: 'Notes' },
      { id: 'g', x: 1000, y: 1000, w: 200, h: 200, title: 'Elsewhere' },
    ]
    const before = [{ id: 'a', ...box(0, 0, 280, 200) }]
    const after = new Map([['a', box(0, 0, 280, 500)]])
    const fitted = layout.fitFrames(frames, before, after)
    assert.equal(fitted[0].h, 500 + 64)
    assert.equal(fitted[1], frames[1], 'untouched frames are the same object')
    assert.equal(layout.fitFrames(frames, before, new Map()), frames)
  })
})
