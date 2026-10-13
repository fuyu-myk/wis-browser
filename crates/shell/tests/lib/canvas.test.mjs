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
  const doc = {
    version: 1,
    view: { x: 0, y: 0, k: 1 },
    cards: [card('a', 0, 0), card('b', 10, 10), card('c', 20, 20)],
    edges: [
      { id: 'ab', from: 'a', to: 'b' },
      { id: 'bc', from: 'b', to: 'c' },
    ],
  }

  test('moving shifts only the chosen cards', () => {
    const moved = canvas.moveCards(doc, new Set(['a', 'c']), 5, -5)
    assert.deepEqual(
      moved.cards.map((c) => [c.id, c.x, c.y]),
      [['a', 5, -5], ['b', 10, 10], ['c', 25, 15]],
    )
    assert.equal(doc.cards[0].x, 0)
  })

  test('removing drops only the chosen cards, and the arrows touching them', () => {
    const left = canvas.removeCards(doc, new Set(['a']))
    assert.deepEqual(left.cards.map((c) => c.id), ['b', 'c'])
    assert.deepEqual(left.edges.map((e) => e.id), ['bc'])
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
  const at = (x, k = 1) => ({ version: 1, view: { x: 0, y: 0, k }, cards: [card('a', x, 0)], edges: [] })

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

describe('arrows', () => {
  const doc = {
    version: 1,
    view: { x: 0, y: 0, k: 1 },
    cards: [card('a'), card('b', 400, 0)],
    edges: [],
  }

  test('stored arrows read back, and ones that cannot stand are dropped', () => {
    const stored = JSON.stringify({
      ...doc,
      edges: [
        { id: 'e1', from: 'a', to: 'b' },
        { id: 'e2', from: 'a', to: 'gone' },
        { id: 'e3', from: 'a', to: 'a' },
        { id: 'e4', from: 'a', to: 'b' },
        { id: 'e5', from: 1, to: 'b' },
        null,
      ],
    })
    assert.deepEqual(canvas.parseDoc(stored).edges, [{ id: 'e1', from: 'a', to: 'b' }])
    assert.deepEqual(canvas.parseDoc(JSON.stringify({ version: 1, cards: [] })).edges, [])
  })

  test('an arrow joins two different cards once', () => {
    const joined = canvas.addEdge(doc, 'a', 'b', 'e1')
    assert.deepEqual(joined.edges, [{ id: 'e1', from: 'a', to: 'b' }])
    assert.equal(canvas.addEdge(joined, 'a', 'b', 'e2'), joined)
    assert.equal(canvas.addEdge(doc, 'a', 'a', 'e3'), doc)
    assert.equal(canvas.addEdge(doc, 'a', 'missing', 'e4'), doc)
    assert.deepEqual(canvas.removeEdge(joined, 'e1').edges, [])
  })

  test('arrows run between the middles of the facing sides', () => {
    const across = canvas.edgeGeometry({ x: 0, y: 0, w: 100, h: 50 }, { x: 300, y: 0, w: 100, h: 50 })
    assert.deepEqual([across.x1, across.y1, across.x2, across.y2], [100, 25, 300, 25])
    const down = canvas.edgeGeometry({ x: 0, y: 0, w: 100, h: 50 }, { x: 0, y: 300, w: 100, h: 50 })
    assert.deepEqual([down.x1, down.y1, down.x2, down.y2], [50, 50, 50, 300])
    const back = canvas.edgeGeometry({ x: 300, y: 0, w: 100, h: 50 }, { x: 0, y: 0, w: 100, h: 50 })
    assert.deepEqual([back.x1, back.x2], [300, 100])
  })

  const lastTwo = (d) => {
    const nums = d.match(/-?\d+(\.\d+)?/g).map(Number)
    const [cx, cy, x, y] = nums.slice(-4)
    return { cx, cy, x, y }
  }

  test('the curve stops a head-length short, aimed so the tip lands on the side middle', () => {
    const g = canvas.edgeGeometry({ x: 0, y: 0, w: 100, h: 50 }, { x: 300, y: 0, w: 100, h: 50 })
    assert.match(g.d, /^M100,25 C/)
    const end = lastTwo(g.d)
    assert.equal(Math.round(Math.hypot(300 - end.x, 25 - end.y)), canvas.ARROW_LENGTH)
    assert.equal(end.y, 25, 'level cards meet head-on')
  })

  test('the head turns with where the source card sits', () => {
    const below = canvas.edgeGeometry({ x: 0, y: 200, w: 100, h: 50 }, { x: 400, y: 0, w: 100, h: 50 })
    const above = canvas.edgeGeometry({ x: 0, y: -200, w: 100, h: 50 }, { x: 400, y: 0, w: 100, h: 50 })
    const up = lastTwo(below.d)
    const down = lastTwo(above.d)
    assert.ok(up.y > 25, 'coming from below, the head points up and in')
    assert.ok(down.y < 25, 'coming from above, the head points down and in')
    assert.ok(400 - up.x > 0 && 400 - down.x > 0, 'both still enter the left side')
  })

  test('cards closer than a head get just the head', () => {
    const touching = canvas.edgeGeometry({ x: 0, y: 0, w: 100, h: 50 }, { x: 105, y: 0, w: 100, h: 50 })
    const end = lastTwo(touching.d)
    assert.deepEqual([end.x, end.y], [100, 25])
  })
})

describe('groups', () => {
  const doc = {
    version: 1,
    view: { x: 0, y: 0, k: 1 },
    cards: [card('a'), card('b', 300), card('c', 600)],
    edges: [],
    frames: [],
  }

  test('grouping ties two or more cards, and selecting one picks them all', () => {
    const grouped = canvas.groupCards(doc, new Set(['a', 'b']), 'g1')
    assert.deepEqual(grouped.cards.map((c) => c.group), ['g1', 'g1', undefined])
    assert.deepEqual([...canvas.expandToGroups(grouped, new Set(['a']))].sort(), ['a', 'b'])
    assert.equal(canvas.groupCards(doc, new Set(['a']), 'g2'), doc, 'one card is not a group')
  })

  test('ungrouping frees every card of the touched groups', () => {
    const grouped = canvas.groupCards(doc, new Set(['a', 'b']), 'g1')
    const freed = canvas.ungroupCards(grouped, new Set(['a']))
    assert.deepEqual(freed.cards.map((c) => c.group), [undefined, undefined, undefined])
  })

  test('grouping is offered for loose cards, ungrouping once a group is picked', () => {
    const grouped = canvas.groupCards(doc, new Set(['a', 'b']), 'g1')
    assert.equal(canvas.canGroup(doc, new Set(['a'])), false)
    assert.equal(canvas.canGroup(doc, new Set(['a', 'b'])), true)
    assert.equal(canvas.canGroup(grouped, new Set(['a', 'b'])), false, 'already one group')
    assert.equal(canvas.canGroup(grouped, new Set(['a', 'b', 'c'])), true)
    assert.equal(canvas.canUngroup(doc, new Set(['a', 'b'])), false)
    assert.equal(canvas.canUngroup(grouped, new Set(['b'])), true)
  })

  test('cards picked out of a group can form their own, leaving a lone card free', () => {
    const grouped = canvas.groupCards(doc, new Set(['a', 'b', 'c']), 'g1')
    assert.equal(canvas.canGroup(grouped, new Set(['a', 'b'])), true, 'part of a group')
    const split = canvas.groupCards(grouped, new Set(['a', 'b']), 'g2')
    assert.deepEqual(split.cards.map((c) => c.group), ['g2', 'g2', 'g1'])
    assert.deepEqual([...canvas.expandToGroups(split, new Set(['c']))], ['c'])
    assert.equal(canvas.canUngroup(split, new Set(['c'])), false, 'one card is not a group')
    assert.deepEqual([...canvas.groupsIn(split, new Set(['c']))], [])
  })

  test('the picked groups are listed with their cards', () => {
    const grouped = canvas.groupCards(
      canvas.groupCards(doc, new Set(['a', 'b']), 'g1'),
      new Set(['c']),
      'g2',
    )
    assert.deepEqual([...canvas.groupsIn(grouped, new Set(['a', 'b', 'c']))], [['g1', ['a', 'b']]])
    assert.deepEqual([...canvas.groupsIn(grouped, new Set(['c']))], [])
  })

  test('groups and frames survive a save', () => {
    const grouped = canvas.addFrame(canvas.groupCards(doc, new Set(['a', 'b']), 'g1'), {
      id: 'f1',
      x: -20,
      y: -40,
      w: 700,
      h: 300,
      title: 'Ideas',
    })
    const back = canvas.parseDoc(canvas.serializeDoc(grouped))
    assert.deepEqual(back.cards.map((c) => c.group), ['g1', 'g1', undefined])
    assert.deepEqual(back.frames, grouped.frames)
  })
})

describe('frames', () => {
  const boxes = [
    { id: 'a', x: 0, y: 0, w: 100, h: 50 },
    { id: 'b', x: 200, y: 100, w: 100, h: 50 },
    { id: 'c', x: 900, y: 900, w: 100, h: 50 },
  ]

  test('a frame around cards leaves a margin on every side', () => {
    const box = canvas.frameAround(boxes.slice(0, 2))
    assert.deepEqual(box, {
      x: -canvas.FRAME_PAD,
      y: -canvas.FRAME_PAD,
      w: 300 + 2 * canvas.FRAME_PAD,
      h: 150 + 2 * canvas.FRAME_PAD,
    })
  })

  test('a tighter margin can be asked for', () => {
    assert.deepEqual(canvas.frameAround(boxes.slice(0, 1), 8), { x: -8, y: -8, w: 116, h: 66 })
  })

  test('frames wholly inside another travel with it', () => {
    const frames = [
      { id: 'outer', x: 0, y: 0, w: 1000, h: 1000, title: '' },
      { id: 'inner', x: 100, y: 100, w: 200, h: 200, title: '' },
      { id: 'across', x: 900, y: 900, w: 200, h: 200, title: '' },
    ]
    assert.deepEqual(canvas.framesInside(frames, frames[0]), ['inner'])
  })

  test('a card is in a frame when its centre is', () => {
    const frame = { id: 'f', x: -10, y: -10, w: 260, h: 120, title: '' }
    assert.deepEqual(canvas.cardsInFrame(boxes, frame), ['a'])
  })

  test('moving a frame carries the cards given with it', () => {
    const doc = {
      version: 1,
      view: { x: 0, y: 0, k: 1 },
      cards: [card('a'), card('b', 900)],
      edges: [],
      frames: [{ id: 'f', x: 0, y: 0, w: 400, h: 300, title: 'F' }],
    }
    const moved = canvas.moveFrames(doc, new Set(['f']), new Set(['a']), 10, 20)
    assert.deepEqual([moved.frames[0].x, moved.frames[0].y], [10, 20])
    assert.deepEqual(moved.cards.map((c) => [c.x, c.y]), [[10, 20], [900, 0]])
  })

  test('frames resize within limits, rename, and go without taking their cards', () => {
    const doc = {
      version: 1,
      view: { x: 0, y: 0, k: 1 },
      cards: [card('a')],
      edges: [],
      frames: [{ id: 'f', x: 0, y: 0, w: 400, h: 300, title: 'F' }],
    }
    const small = canvas.resizeFrame(doc, 'f', 5, 5)
    assert.deepEqual([small.frames[0].w, small.frames[0].h], [canvas.MIN_FRAME_W, canvas.MIN_FRAME_H])
    assert.equal(canvas.renameFrame(doc, 'f', '  Plan  ').frames[0].title, 'Plan')
    assert.equal(canvas.renameFrame(doc, 'f', '   ').frames[0].title, 'F', 'a blank title keeps the old one')
    const gone = canvas.removeFrame(doc, 'f')
    assert.deepEqual([gone.frames.length, gone.cards.length], [0, 1])
  })

  test('edits that change nothing hand back the same document', () => {
    const doc = {
      version: 1,
      view: { x: 0, y: 0, k: 1 },
      cards: [card('a')],
      edges: [],
      frames: [{ id: 'f', x: 0, y: 0, w: 400, h: 300, title: 'F' }],
    }
    assert.equal(canvas.removeCards(doc, new Set(['gone'])), doc)
    assert.equal(canvas.removeEdge(doc, 'gone'), doc)
    assert.equal(canvas.removeFrame(doc, 'gone'), doc)
    assert.equal(canvas.moveFrames(doc, new Set(['f']), new Set(['a']), 0, 0), doc)
    assert.equal(canvas.resizeFrame(doc, 'f', 400, 300), doc)
    assert.equal(canvas.resizeFrame(doc, 'gone', 500, 500), doc)
    assert.equal(canvas.renameFrame(doc, 'f', 'F'), doc)
  })

  test('stored frames that cannot stand are dropped', () => {
    const stored = JSON.stringify({
      version: 1,
      cards: [],
      frames: [{ id: 'f', x: 0, y: 0, w: 400, h: 300, title: 'ok' }, { id: 'g', x: 'no' }, null, { id: 'f', x: 0, y: 0, w: 400, h: 300, title: 'dup' }],
    })
    assert.deepEqual(canvas.parseDoc(stored).frames.map((f) => f.title), ['ok'])
  })
})
