// The canvas document (`lib/canvas.ts`) and its channel to Rust (`lib/canvasChannel.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import * as canvas from '../../src/lib/canvas.ts'
import { createCanvasChannel } from '../../src/lib/canvasChannel.ts'
import { followPushed } from '../../src/lib/canvasPushed.ts'

const card = (id, x = 0, y = 0, text = 'note') => ({ id, x, y, w: canvas.CARD_WIDTH, text })

describe('documents', () => {
  test('an empty or unreadable document starts a blank canvas', () => {
    for (const text of ['{}', '', 'not json', '[]', 'null', '{"version":99}']) {
      assert.deepEqual(canvas.parseDoc(text), canvas.emptyDoc(), text)
    }
  })

  test('stored cards are read back, and malformed ones dropped', () => {
    const stored = JSON.stringify({
      version: 1,
      view: { x: 10, y: -20, k: 2 },
      cards: [card('a', 1, 2, '# Hi'), { id: 'b', x: 'nope' }, null, card('', 0, 0), card('c', 5, 6)],
    })
    const doc = canvas.parseDoc(stored)
    assert.deepEqual(doc.view, { x: 10, y: -20, k: 2 })
    assert.deepEqual(
      doc.cards.map((c) => c.id),
      ['a', 'c'],
    )
    assert.equal(canvas.parseDoc(canvas.serializeDoc(doc)).cards[0].text, '# Hi')
  })

  test('a stored view is clamped to the zoom range, and card widths to theirs', () => {
    const doc = canvas.parseDoc(
      JSON.stringify({ version: 1, view: { x: 0, y: 0, k: 50 }, cards: [{ ...card('a'), w: 5 }] }),
    )
    assert.equal(doc.view.k, canvas.MAX_ZOOM)
    assert.equal(doc.cards[0].w, canvas.MIN_CARD_WIDTH)
  })

  test('cards are added, changed and removed without touching the original', () => {
    const start = canvas.emptyDoc()
    const one = canvas.addCard(start, card('a'))
    const moved = canvas.updateCard(one, 'a', { x: 40, text: 'moved' })
    const gone = canvas.removeCard(moved, 'a')
    assert.equal(start.cards.length, 0)
    assert.deepEqual(one.cards[0], card('a'))
    assert.deepEqual(moved.cards[0], { ...card('a'), x: 40, text: 'moved' })
    assert.equal(gone.cards.length, 0)
    assert.equal(canvas.updateCard(one, 'missing', { x: 1 }), one)
  })

  test('a card made at a point is centred on it', () => {
    const made = canvas.newCard(500, 300, 'id-1')
    assert.deepEqual(made, { id: 'id-1', x: 500 - canvas.CARD_WIDTH / 2, y: 300 - 20, w: canvas.CARD_WIDTH, text: '' })
  })
})

describe('the view', () => {
  test('zooming keeps the point under the pointer still', () => {
    const view = { x: 100, y: 50, k: 1 }
    const zoomed = canvas.zoomAt(view, 300, 250, 2)
    const before = canvas.toCanvas(view, 300, 250)
    const after = canvas.toCanvas(zoomed, 300, 250)
    assert.equal(zoomed.k, 2)
    assert.ok(Math.abs(before.x - after.x) < 1e-9 && Math.abs(before.y - after.y) < 1e-9)
  })

  test('zoom stops at its limits', () => {
    assert.equal(canvas.zoomAt({ x: 0, y: 0, k: 1 }, 0, 0, 1000).k, canvas.MAX_ZOOM)
    assert.equal(canvas.zoomAt({ x: 0, y: 0, k: 1 }, 0, 0, 0.0001).k, canvas.MIN_ZOOM)
  })
})

describe('the channel', () => {
  const setup = () => {
    const posted = []
    const channel = createCanvasChannel((text) => posted.push(JSON.parse(text)))
    return { posted, channel }
  }

  test('requests carry an id, and replies settle the matching request', async () => {
    const { posted, channel } = setup()
    const listing = channel.list()
    const loading = channel.load('b1')
    assert.deepEqual(posted, [
      { id: 1, op: 'list' },
      { id: 2, op: 'load', canvas: 'b1' },
    ])
    channel.reply({ id: 2, ok: true, value: { id: 'b1', name: 'Plans', doc: '{}', updatedAt: 1 } })
    channel.reply({ id: 1, ok: true, value: [] })
    assert.equal((await loading).name, 'Plans')
    assert.deepEqual(await listing, [])
  })

  test('a failed reply rejects with its message', async () => {
    const { channel } = setup()
    const saving = channel.save('b1', '{}')
    channel.reply({ id: 1, ok: false, error: 'that canvas no longer exists' })
    await assert.rejects(saving, /no longer exists/)
  })

  test('replies that match nothing, or are malformed, are ignored', async () => {
    const { channel } = setup()
    const creating = channel.create('Trip')
    for (const junk of [null, 'text', { id: 99, ok: true }, { id: null, ok: false }, { ok: true }]) {
      channel.reply(junk)
    }
    channel.reply({ id: 1, ok: true, value: { id: 'n', name: 'Trip', updatedAt: 1 } })
    assert.equal((await creating).name, 'Trip')
  })

  test('without the native channel every request fails at once', async () => {
    const channel = createCanvasChannel(null)
    await assert.rejects(channel.list(), /not available/)
  })
})

describe('values Rust pushes in', () => {
  const isColor = (v) => /^#[0-9a-f]{6}$/i.test(v)

  test('a value already set is applied, and later ones as they arrive', () => {
    const host = { __wisSwipeAccent: '#112233' }
    const applied = []
    followPushed(host, '__wisSwipeAccent', (c) => applied.push(c), isColor)
    host.__wisSwipeAccent = '#445566'
    assert.deepEqual(applied, ['#112233', '#445566'])
    assert.equal(host.__wisSwipeAccent, '#445566', 'other readers still see it')
  })

  test('values that fail the check are ignored', () => {
    const host = {}
    const applied = []
    followPushed(host, '__wisCanvasTheme', (t) => applied.push(t), (v) => v === 'dark' || v === 'light')
    host.__wisCanvasTheme = 'sepia'
    host.__wisCanvasTheme = 42
    host.__wisCanvasTheme = 'light'
    assert.deepEqual(applied, ['light'])
    assert.equal(host.__wisCanvasTheme, 'light')
  })
})

describe('finding canvases', () => {
  const list = [
    { id: 'a', name: 'Trip plans', updatedAt: 1 },
    { id: 'b', name: 'Rust notes', updatedAt: 3 },
    { id: 'c', name: 'Reading list', updatedAt: 2 },
  ]

  test('with no query, the most recently changed come first', () => {
    assert.deepEqual(canvas.matchCanvases(list, '  ').map((c) => c.id), ['b', 'c', 'a'])
  })

  test('a query keeps fuzzy matches, best first', () => {
    assert.deepEqual(canvas.matchCanvases(list, 'notes').map((c) => c.id), ['b'])
    assert.deepEqual(canvas.matchCanvases(list, 'r').map((c) => c.id), ['b', 'c', 'a'])
    assert.deepEqual(canvas.matchCanvases(list, 'trip').map((c) => c.id), ['a'])
    assert.deepEqual(canvas.matchCanvases(list, 'zzz'), [])
  })
})

describe('canvas names', () => {
  test('names are tidied as Rust stores them', () => {
    assert.equal(canvas.tidyName('  Trip \n plans  '), 'Trip plans')
    assert.equal(canvas.tidyName('\t '), '')
    assert.equal(Array.from(canvas.tidyName('😀'.repeat(200))).length, 120, 'capped by character')
  })
})

describe('editing several cards', () => {
  const doc = { version: 1, view: { x: 0, y: 0, k: 1 }, cards: [card('a', 0, 0), card('b', 10, 10), card('c', 20, 20)] }

  test('moving shifts only the chosen cards', () => {
    const moved = canvas.moveCards(doc, new Set(['a', 'c']), 5, -5)
    assert.deepEqual(
      moved.cards.map((c) => [c.id, c.x, c.y]),
      [['a', 5, -5], ['b', 10, 10], ['c', 25, 15]],
    )
    assert.equal(doc.cards[0].x, 0)
  })

  test('removing drops only the chosen cards', () => {
    assert.deepEqual(canvas.removeCards(doc, new Set(['a', 'b'])).cards.map((c) => c.id), ['c'])
  })
})

describe('box selection', () => {
  test('a box is the same whichever way it was dragged', () => {
    assert.deepEqual(canvas.boxBetween(10, 20, 4, 2), { x: 4, y: 2, w: 6, h: 18 })
  })

  test('cards touching the box are picked', () => {
    const boxes = [
      { id: 'in', x: 10, y: 10, w: 20, h: 20 },
      { id: 'edge', x: 45, y: 10, w: 20, h: 20 },
      { id: 'out', x: 100, y: 100, w: 20, h: 20 },
    ]
    assert.deepEqual(canvas.cardsTouching(boxes, { x: 0, y: 0, w: 50, h: 50 }), ['in', 'edge'])
    assert.deepEqual(canvas.cardsTouching(boxes, { x: 200, y: 200, w: 5, h: 5 }), [])
  })
})

describe('undo history', () => {
  const at = (x, k = 1) => ({ version: 1, view: { x: 0, y: 0, k }, cards: [card('a', x, 0)] })

  test('undo and redo step through recorded documents, keeping the current view', () => {
    let history = canvas.record(canvas.emptyHistory(), at(0))
    history = canvas.record(history, at(1))
    const current = at(2, 3)

    const back = canvas.undo(history, current)
    assert.equal(back.doc.cards[0].x, 1)
    assert.equal(back.doc.view.k, 3, 'the view stays where it is')

    const again = canvas.undo(back.history, back.doc)
    assert.equal(again.doc.cards[0].x, 0)
    assert.equal(canvas.undo(again.history, again.doc), null)

    const forward = canvas.redo(again.history, again.doc)
    assert.equal(forward.doc.cards[0].x, 1)
  })

  test('a new change clears redo, and history keeps a bounded past', () => {
    let history = canvas.record(canvas.emptyHistory(), at(0))
    const back = canvas.undo(history, at(1))
    history = canvas.record(back.history, back.doc)
    assert.equal(canvas.redo(history, at(5)), null)

    let long = canvas.emptyHistory()
    for (let i = 0; i < 500; i += 1) long = canvas.record(long, at(i))
    assert.equal(long.past.length, canvas.HISTORY_LIMIT)
    assert.equal(long.past[long.past.length - 1].cards[0].x, 499)
  })
})
