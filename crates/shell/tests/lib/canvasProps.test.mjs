// What the canvas props panel shows and changes (`lib/canvasProps.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import * as canvas from '../../src/lib/canvas.ts'
import * as props from '../../src/lib/canvasProps.ts'

const note = (id, x, y, text = id, w = 280) => ({ id, x, y, w, text })
const html = (id, x, y, w = 480, h = 320) => ({ id, x, y, w, h, kind: 'html', text: `<title>${id}</title>` })
const doc = (cards = [], extra = {}) => ({ version: 1, view: { x: 0, y: 0, k: 1 }, cards, edges: [], frames: [], ...extra })
const heightOf = (c) => (c.kind === 'html' ? c.h : 100)
const ids = (...list) => new Set(list)

describe('what the panel shows', () => {
  test('nothing for an empty selection', () => {
    assert.equal(props.propsView(doc([note('a', 0, 0)]), ids(), null, heightOf), null)
  })

  test('one card with its kind, title and box; a note’s height is its own', () => {
    const view = props.propsView(doc([note('a', 10, 20, '# Plan\nmore')]), ids('a'), null, heightOf)
    assert.deepEqual(view, {
      kind: 'cards',
      ids: ['a'],
      label: 'Note',
      title: 'Plan',
      box: { x: 10, y: 20, w: 280, h: 100 },
      w: 280,
      h: 100,
      sized: false,
    })
    const visual = props.propsView(doc([html('v', 0, 0)]), ids('v'), null, heightOf)
    assert.equal(visual.label, 'Visual')
    assert.equal(visual.sized, true)
  })

  test('several cards with their bounds and shared or mixed sizes', () => {
    const d = doc([note('a', 0, 0), note('b', 400, 50), html('v', 0, 300, 280)])
    const view = props.propsView(d, ids('a', 'b', 'v'), null, heightOf)
    assert.equal(view.label, '3 cards')
    assert.deepEqual(view.box, { x: 0, y: 0, w: 680, h: 620 })
    assert.equal(view.w, 280, 'all 280 wide')
    assert.equal(view.h, null, 'heights differ')
    assert.equal(view.sized, false, 'not every card takes a height')
  })

  test('a picked frame or arrow shows instead of cards', () => {
    const d = doc([note('a', 0, 0, 'Alpha'), note('b', 400, 0, 'Beta')], {
      frames: [{ id: 'f', x: -20, y: -20, w: 800, h: 200, title: 'Ideas' }],
      edges: [{ id: 'e', from: 'a', to: 'b' }],
    })
    assert.deepEqual(props.propsView(d, ids(), { kind: 'frame', id: 'f' }, heightOf), {
      kind: 'frame',
      id: 'f',
      title: 'Ideas',
      box: { x: -20, y: -20, w: 800, h: 200 },
    })
    assert.deepEqual(props.propsView(d, ids(), { kind: 'edge', id: 'e' }, heightOf), {
      kind: 'edge',
      id: 'e',
      from: 'Alpha',
      to: 'Beta',
    })
    assert.equal(props.propsView(d, ids(), { kind: 'frame', id: 'gone' }, heightOf), null)
  })
})

describe('what the panel changes', () => {
  test('X and Y move the whole selection so its bounds start there', () => {
    const d = doc([note('a', 0, 0), note('b', 400, 50)])
    const moved = props.setCards(d, ['a', 'b'], 'x', 100, heightOf)
    assert.deepEqual(moved.cards.map((c) => [c.x, c.y]), [[100, 0], [500, 50]])
    const down = props.setCards(d, ['a', 'b'], 'y', -40, heightOf)
    assert.deepEqual(down.cards.map((c) => c.y), [-40, 10])
    assert.equal(props.setCards(d, ['a', 'b'], 'x', 0, heightOf), d, 'no change, same document')
  })

  test('W sets every width within limits; H sizes only visuals', () => {
    const d = doc([note('a', 0, 0), html('v', 0, 300)])
    const wide = props.setCards(d, ['a', 'v'], 'w', 5000, heightOf)
    assert.deepEqual(wide.cards.map((c) => c.w), [canvas.MAX_CARD_WIDTH, canvas.MAX_CARD_WIDTH])
    const tall = props.setCards(d, ['a', 'v'], 'h', 10, heightOf)
    assert.equal('h' in tall.cards[0], false, 'a note keeps following its text')
    assert.equal(tall.cards[1].h, canvas.MIN_CARD_HEIGHT)
  })

  test('a frame moves with what it holds, and resizes within limits', () => {
    const d = doc([note('in', 0, 0), note('out', 2000, 0)], {
      frames: [
        { id: 'f', x: -20, y: -20, w: 400, h: 200, title: 'F' },
        { id: 'inner', x: 0, y: 120, w: 160, h: 40, title: 'I' },
      ],
    })
    const moved = props.setFrame(d, 'f', 'x', 80, heightOf)
    assert.deepEqual(moved.frames.map((f) => f.x), [80, 100], 'the frame and the frame inside it')
    assert.deepEqual(moved.cards.map((c) => c.x), [100, 2000], 'its card comes along; others stay')
    const sized = props.setFrame(d, 'f', 'w', 1, heightOf)
    assert.equal(sized.frames[0].w, canvas.MIN_FRAME_W)
    assert.equal(props.setFrame(d, 'nope', 'x', 5, heightOf), d)
  })

  test('a frame takes a tidy new name, never a blank one', () => {
    const d = doc([], { frames: [{ id: 'f', x: 0, y: 0, w: 400, h: 200, title: 'Old' }] })
    assert.equal(props.setFrameTitle(d, 'f', '  New name ').frames[0].title, 'New name')
    assert.equal(props.setFrameTitle(d, 'f', '   '), d)
    assert.equal(props.setFrameTitle(d, 'f', 'Old'), d)
  })

  test('far-flung or broken numbers are pulled back or ignored', () => {
    const d = doc([note('a', 0, 0)])
    assert.equal(props.setCards(d, ['a'], 'x', 1e15, heightOf).cards[0].x, canvas.MAX_COORD)
    assert.equal(props.setCards(d, ['a'], 'x', Number.NaN, heightOf), d)
  })
})

describe('typed numbers', () => {
  test('read plain or signed numbers, rounded; anything else is nothing', () => {
    assert.equal(props.readNumber(' 120 '), 120)
    assert.equal(props.readNumber('-40.6'), -41)
    assert.equal(props.readNumber(''), null)
    assert.equal(props.readNumber('12px'), null)
    assert.equal(props.readNumber('1e400'), null)
  })
})
