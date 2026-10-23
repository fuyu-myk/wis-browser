// Aligning, distributing and tidying selected cards (`features/canvas/lib/canvasArrange.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import * as arrange from '../../src/features/canvas/lib/canvasArrange.ts'
import { GAP } from '../../src/features/canvas/lib/canvasLayout.ts'

const note = (id, x, y, w = 200, h = 100) => ({ id, x, y, w, text: id, h })
const doc = (cards) => ({ version: 1, view: { x: 0, y: 0, k: 1 }, cards, edges: [], frames: [] })
/** Notes as tall as the test says; the canvas measures them. */
const heightOf = (c) => c.h
const at = (d, id) => {
  const c = d.cards.find((card) => card.id === id)
  return [c.x, c.y]
}
const overlapping = (d) => {
  const b = d.cards.map((c) => ({ id: c.id, x: c.x, y: c.y, w: c.w, h: c.h }))
  for (let i = 0; i < b.length; i++)
    for (let j = i + 1; j < b.length; j++)
      if (b[i].x < b[j].x + b[j].w && b[j].x < b[i].x + b[i].w && b[i].y < b[j].y + b[j].h && b[j].y < b[i].y + b[i].h)
        return `${b[i].id}/${b[j].id}`
  return null
}

describe('aligning', () => {
  const column = doc([note('a', 40, 0), note('b', 0, 200, 300), note('c', 90, 400, 100)])

  test('lines up left, centre or right edges', () => {
    const left = arrange.alignCards(column, ['a', 'b', 'c'], 'left', heightOf)
    assert.deepEqual(left.cards.map((c) => c.x), [0, 0, 0])
    const right = arrange.alignCards(column, ['a', 'b', 'c'], 'right', heightOf)
    assert.deepEqual(right.cards.map((c) => c.x + c.w), [300, 300, 300])
    const centre = arrange.alignCards(column, ['a', 'b', 'c'], 'centre', heightOf)
    assert.deepEqual(centre.cards.map((c) => c.x + c.w / 2), [150, 150, 150])
    assert.deepEqual(left.cards.map((c) => c.y), [0, 200, 400], 'already apart, so nothing else moves')
  })

  test('lines up top, middle or bottom edges', () => {
    const row = doc([note('a', 0, 30), note('b', 300, 0, 200, 200), note('c', 600, 60)])
    assert.deepEqual(arrange.alignCards(row, ['a', 'b', 'c'], 'top', heightOf).cards.map((c) => c.y), [0, 0, 0])
    const bottom = arrange.alignCards(row, ['a', 'b', 'c'], 'bottom', heightOf)
    assert.deepEqual(bottom.cards.map((c) => c.y + c.h), [200, 200, 200])
    const middle = arrange.alignCards(row, ['a', 'b', 'c'], 'middle', heightOf)
    assert.deepEqual(middle.cards.map((c) => c.y + c.h / 2), [100, 100, 100])
  })

  test('cards it would pile up spread along the other axis, in order', () => {
    const row = doc([note('a', 0, 0), note('b', 300, 0), note('c', 600, 0)])
    const stacked = arrange.alignCards(row, ['a', 'b', 'c'], 'left', heightOf)
    assert.equal(overlapping(stacked), null)
    assert.deepEqual(['a', 'b', 'c'].map((id) => at(stacked, id)), [[0, 0], [0, 100 + GAP], [0, 200 + 2 * GAP]])
    const column = doc([note('a', 0, 0), note('b', 0, 150), note('c', 0, 300)])
    const lined = arrange.alignCards(column, ['a', 'b', 'c'], 'top', heightOf)
    assert.equal(overlapping(lined), null)
    assert.deepEqual(lined.cards.map((c) => c.y), [0, 0, 0])
  })

  test('leaves other cards, and needs two', () => {
    const d = doc([note('a', 40, 0), note('b', 0, 200), note('other', 999, 999)])
    const out = arrange.alignCards(d, ['a', 'b'], 'left', heightOf)
    assert.deepEqual(at(out, 'other'), [999, 999])
    assert.equal(arrange.alignCards(d, ['a'], 'left', heightOf), d)
    assert.equal(arrange.alignCards(arrange.alignCards(d, ['a', 'b'], 'left', heightOf), ['a', 'b'], 'left', heightOf).cards[0].x, 0)
    const done = arrange.alignCards(d, ['a', 'b'], 'left', heightOf)
    assert.equal(arrange.alignCards(done, ['a', 'b'], 'left', heightOf), done, 'no change, same document')
  })
})

describe('distributing', () => {
  test('evens the gaps between the outer cards', () => {
    const d = doc([note('a', 0, 0), note('c', 1000, 0), note('b', 100, 50)])
    const out = arrange.distributeCards(d, ['a', 'b', 'c'], 'across', heightOf)
    assert.deepEqual(['a', 'b', 'c'].map((id) => at(out, id)[0]), [0, 500, 1000])
    assert.deepEqual(at(out, 'b')[1], 50, 'the other axis stays')
    const v = doc([note('a', 0, 0, 200, 100), note('b', 0, 150, 200, 300), note('c', 0, 900, 200, 100)])
    const down = arrange.distributeCards(v, ['a', 'b', 'c'], 'down', heightOf)
    assert.deepEqual(down.cards.map((c) => c.y), [0, 350, 900])
  })

  test('cards too close spread to the gap instead of overlapping', () => {
    const d = doc([note('a', 0, 0), note('b', 50, 0), note('c', 100, 0)])
    const out = arrange.distributeCards(d, ['a', 'b', 'c'], 'across', heightOf)
    assert.deepEqual(out.cards.map((c) => c.x), [0, 200 + GAP, 400 + 2 * GAP])
    assert.equal(overlapping(out), null)
  })

  test('needs three', () => {
    const d = doc([note('a', 0, 0), note('b', 500, 0)])
    assert.equal(arrange.distributeCards(d, ['a', 'b'], 'across', heightOf), d)
  })
})

describe('tidying', () => {
  test('lays the cards out in a grid in reading order, sized to the cards', () => {
    const d = doc([note('d', 900, 700), note('a', 10, 20, 300, 150), note('c', 50, 600), note('b', 700, 0)])
    const out = arrange.tidyCards(d, ['a', 'b', 'c', 'd'], heightOf)
    assert.equal(overlapping(out), null)
    assert.deepEqual(at(out, 'a'), [10, 20], 'the first card anchors the grid')
    assert.deepEqual(at(out, 'b'), [10 + 300 + GAP, 20])
    assert.deepEqual(at(out, 'c'), [10, 20 + 150 + GAP])
    assert.deepEqual(at(out, 'd'), [10 + 300 + GAP, 20 + 150 + GAP])
  })

  test('the grid moves clear of cards that are not in it', () => {
    const d = doc([note('a', 0, 0), note('b', 900, 0), note('other', 250, 0)])
    const out = arrange.tidyCards(d, ['a', 'b'], heightOf)
    assert.equal(overlapping(out), null)
    assert.deepEqual(at(out, 'other'), [250, 0])
    assert.equal(at(out, 'b')[0] - at(out, 'a')[0], 200 + GAP, 'the grid keeps its shape')
  })

  test('needs two', () => {
    const d = doc([note('a', 0, 0)])
    assert.equal(arrange.tidyCards(d, ['a'], heightOf), d)
  })
})
