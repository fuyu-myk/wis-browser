// The canvas document (`lib/canvas.ts`) and its channel to Rust (`lib/canvasChannel.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import * as canvas from '../../src/lib/canvas.ts'
import { createCanvasChannel } from '../../src/lib/canvasChannel.ts'
import { followThemeAccent } from '../../src/lib/canvasAccent.ts'

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

describe('the theme accent', () => {
  const isColor = (v) => /^#[0-9a-f]{6}$/i.test(v)

  test('the accent already set is applied, and later ones as they arrive', () => {
    const host = { __wisSwipeAccent: '#112233' }
    const applied = []
    followThemeAccent(host, (c) => applied.push(c), isColor)
    host.__wisSwipeAccent = '#445566'
    assert.deepEqual(applied, ['#112233', '#445566'])
    assert.equal(host.__wisSwipeAccent, '#445566', 'the swipe indicator still reads it')
  })

  test('values that are not colours are ignored', () => {
    const host = {}
    const applied = []
    followThemeAccent(host, (c) => applied.push(c), isColor)
    host.__wisSwipeAccent = 'red; background: url(x)'
    host.__wisSwipeAccent = 42
    assert.deepEqual(applied, [])
    assert.equal(host.__wisSwipeAccent, undefined)
  })
})
