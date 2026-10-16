// The canvas agent's tools (`lib/canvasAgent.ts`): what they read, how they
// change a document, and where new cards land.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import * as canvas from '../../src/lib/canvas.ts'
import * as agent from '../../src/lib/canvasAgent.ts'

const note = (id, x, y, text = 'note') => ({ id, x, y, w: 280, text })
const doc = (cards = [], extra = {}) => ({
  version: 1,
  view: { x: 0, y: 0, k: 1 },
  cards,
  edges: [],
  frames: [],
  ...extra,
})
let counter = 0
const ctx = (over = {}) => ({
  selection: [],
  view: { x: 0, y: 0, w: 1200, h: 800 },
  heightOf: () => 60,
  newId: () => `n${++counter}`,
  ...over,
})
const call = (name, args) => ({ id: 'c1', name, arguments: JSON.stringify(args) })
const overlaps = (a, b) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h

describe('reading', () => {
  test('read_canvas lists cards by title with frames, arrows, selection and the view', () => {
    const d = doc([note('a', 0, 0, '# Plan\nmore'), { ...canvas.newHtmlCard(500, 500, 'h'), text: '<title>Chart</title>' }], {
      edges: [{ id: 'e', from: 'a', to: 'h' }],
      frames: [{ id: 'f', x: -20, y: -20, w: 400, h: 200, title: 'Ideas' }],
    })
    const out = agent.runCanvasTool(d, call('read_canvas', {}), ctx({ selection: ['a'] }))
    const read = JSON.parse(out.result)
    assert.equal(out.doc, d, 'reading changes nothing')
    assert.deepEqual(read.cards.map((c) => [c.id, c.kind, c.title]), [['h', 'html', 'Chart'], ['a', 'note', 'Plan']], 'nearest the view first')
    assert.deepEqual(read.frames[0].title, 'Ideas')
    assert.deepEqual(read.arrows, [{ from: 'a', to: 'h' }])
    assert.deepEqual(read.selected, ['a'])
    assert.deepEqual(read.visible, { x: 0, y: 0, w: 1200, h: 800 })
  })

  test('read_cards returns full text and names the missing', () => {
    const d = doc([note('a', 0, 0, 'alpha body')])
    const read = JSON.parse(agent.runCanvasTool(d, call('read_cards', { ids: ['a', 'zz'] }), ctx()).result)
    assert.deepEqual(read.cards, [{ id: 'a', kind: 'note', text: 'alpha body' }])
    assert.deepEqual(read.missing, ['zz'])
  })

  test('bad arguments and unknown tools come back as errors the model can read', () => {
    const d = doc()
    for (const bad of [{ id: 'c', name: 'read_cards', arguments: '{nope' }, call('rm_rf', {}), call('read_cards', { ids: 'a' })]) {
      const out = agent.runCanvasTool(d, bad, ctx())
      assert.equal(out.doc, d)
      assert.match(out.result, /^Error: /, bad.name)
    }
  })
})

describe('changing the canvas', () => {
  test('create_cards adds notes and HTML cards in free space near the view', () => {
    const d = doc([note('a', 460, 250, 'in the middle')])
    const out = agent.runCanvasTool(
      d,
      call('create_cards', { cards: [{ text: 'one' }, { text: 'two' }, { kind: 'html', text: '<p>x</p>' }] }),
      ctx(),
    )
    const made = out.doc.cards.slice(1)
    assert.deepEqual(made.map((c) => c.kind ?? 'note'), ['note', 'note', 'html'])
    const boxes = out.doc.cards.map((c) => ({ x: c.x, y: c.y, w: c.w, h: c.h ?? 60 }))
    for (let i = 0; i < boxes.length; i++)
      for (let j = i + 1; j < boxes.length; j++) assert.ok(!overlaps(boxes[i], boxes[j]), `${i} overlaps ${j}`)
    for (const c of made) assert.ok(c.x > -1200 && c.x < 2400 && c.y > -800 && c.y < 1600, 'near the view')
    assert.deepEqual(JSON.parse(out.result).created.map((c) => c.id), made.map((c) => c.id))
    assert.equal(out.summary, 'Added 2 notes and a visual')
  })

  test('each step names the cards it touched', () => {
    const d = doc([note('a', 0, 0, '# Plan\nbody'), note('b', 400, 0, 'Budget')])
    const made = agent.runCanvasTool(d, call('create_cards', { cards: [{ text: '## Day 1\narrive' }, { kind: 'html', text: '<title>Timeline</title>' }] }), ctx())
    assert.deepEqual(made.details, ['Day 1', 'Timeline (visual)'])
    assert.equal(made.summary, 'Added a note and a visual')
    const linked = agent.runCanvasTool(d, call('connect_cards', { links: [{ from: 'a', to: 'b' }] }), ctx())
    assert.deepEqual(linked.details, ['Plan to Budget'])
    const gone = agent.runCanvasTool(d, call('delete_cards', { ids: ['b'] }), ctx())
    assert.deepEqual(gone.details, ['Budget'])
    const edited = agent.runCanvasTool(d, call('update_cards', { cards: [{ id: 'a', text: 'Renamed' }] }), ctx())
    assert.deepEqual(edited.details, ['Renamed'])
    assert.deepEqual(agent.runCanvasTool(d, call('read_canvas', {}), ctx()).details, [])
  })

  test('cards given a place keep it, and `near` puts one beside its anchor', () => {
    const d = doc([note('a', 0, 0)])
    const out = agent.runCanvasTool(
      d,
      call('create_cards', { cards: [{ text: 'here', x: 900, y: 900 }, { text: 'beside', near: 'a' }] }),
      ctx(),
    )
    const [, here, beside] = out.doc.cards
    assert.deepEqual([here.x, here.y], [900, 900])
    assert.ok(beside.x >= 280 && beside.x < 700 && Math.abs(beside.y) < 300, `${beside.x},${beside.y}`)
  })

  test('update, delete, connect and frame do what they say and report what they skipped', () => {
    const d = doc([note('a', 0, 0), note('b', 400, 0), { ...canvas.newHtmlCard(0, 600, 'h') }])
    let out = agent.runCanvasTool(d, call('update_cards', { cards: [{ id: 'a', text: 'new', x: 10 }, { id: 'h', h: 500 }, { id: 'zz' }] }), ctx())
    const [a, , h] = out.doc.cards
    assert.deepEqual([a.text, a.x, h.h], ['new', 10, 500])
    assert.deepEqual(JSON.parse(out.result).missing, ['zz'])

    out = agent.runCanvasTool(out.doc, call('connect_cards', { links: [{ from: 'a', to: 'b' }, { from: 'a', to: 'a' }] }), ctx())
    assert.equal(out.doc.edges.length, 1)
    assert.equal(JSON.parse(out.result).skipped.length, 1)

    out = agent.runCanvasTool(out.doc, call('frame_cards', { ids: ['a', 'b'], title: 'Pair' }), ctx())
    assert.equal(out.doc.frames[0].title, 'Pair')
    assert.ok(out.doc.frames[0].x < 0 && out.doc.frames[0].w > 680)

    out = agent.runCanvasTool(out.doc, call('delete_cards', { ids: ['b', 'zz'] }), ctx())
    assert.deepEqual(out.doc.cards.map((c) => c.id), ['a', 'h'])
    assert.equal(out.doc.edges.length, 0)
    assert.equal(out.summary, 'Deleted a note')
  })

  test('steps name what they made by kind', () => {
    const d = doc([note('a', 0, 0, 'Plan'), { ...canvas.newHtmlCard(0, 600, 'h'), text: '<title>Chart</title>' }])
    const html = (n) => Array.from({ length: n }, () => ({ kind: 'html', text: '<p>x</p>' }))
    assert.equal(agent.runCanvasTool(d, call('create_cards', { cards: html(1) }), ctx()).summary, 'Created a visual')
    assert.equal(agent.runCanvasTool(d, call('create_cards', { cards: html(3) }), ctx()).summary, 'Created 3 visuals')
    assert.equal(agent.runCanvasTool(d, call('create_cards', { cards: [{ text: 'x' }] }), ctx()).summary, 'Added a note')
    assert.equal(agent.runCanvasTool(d, call('update_cards', { cards: [{ id: 'a', x: 5 }, { id: 'h', x: 5 }] }), ctx()).summary, 'Edited a note and a visual')
    assert.equal(agent.runCanvasTool(d, call('read_cards', { ids: ['h'] }), ctx()).summary, 'Read a visual')
    assert.equal(agent.runCanvasTool(d, call('connect_cards', { links: [{ from: 'a', to: 'h' }] }), ctx()).summary, 'Drew an arrow')
  })

  test('a step that did nothing says so, and why when it can', () => {
    const d = doc([note('a', 0, 0)])
    const full = doc(Array.from({ length: canvas.MAX_CARDS }, (_, i) => note(`k${i}`, (i % 70) * 400, Math.floor(i / 70) * 200)))
    assert.equal(agent.runCanvasTool(full, call('create_cards', { cards: [{ text: 'x', x: 0, y: -9000 }] }), ctx()).summary, 'No cards added: the canvas is full')
    const some = agent.runCanvasTool(d, call('create_cards', { cards: [{ kind: 'html', text: 'x'.repeat(400_000) }, { kind: 'html', text: 'x'.repeat(400_000) }, { kind: 'html', text: 'x'.repeat(400_000) }] }), ctx())
    assert.equal(some.summary, 'Created 2 visuals (1 skipped)')
    assert.equal(agent.runCanvasTool(d, call('connect_cards', { links: [{ from: 'a', to: 'a' }] }), ctx()).summary, 'No arrows drawn')
    assert.equal(agent.runCanvasTool(d, call('update_cards', { cards: [{ id: 'zz' }] }), ctx()).summary, 'No cards found to edit')
    assert.equal(agent.runCanvasTool(d, call('delete_cards', { ids: ['zz'] }), ctx()).summary, 'No cards found to delete')
    assert.equal(agent.runCanvasTool(d, call('read_cards', { ids: ['zz'] }), ctx()).summary, 'No cards found to read')
  })

  test('a note will not take a web page, so its Markdown is never swapped for HTML', () => {
    const d = doc([note('a', 0, 0, '- step one'), { ...canvas.newHtmlCard(0, 600, 'h') }])
    for (const page of ['<!doctype html><p>chart</p>', '  <html><body>x</body></html>', '<div id=c></div>\n<script>draw()</script>', '<style>p{}</style><p>x</p>']) {
      const out = agent.runCanvasTool(d, call('update_cards', { cards: [{ id: 'a', text: page }] }), ctx())
      assert.equal(out.doc, d, page)
      assert.match(out.result, /^Error: .*create_cards/, page)
    }
    const refused = agent.runCanvasTool(d, call('update_cards', { cards: [{ id: 'a', text: '<!doctype html>' }] }), ctx())
    assert.equal(refused.summary, 'Kept the notes as they were', 'the user sees a plain line')
    assert.equal(agent.runCanvasTool(d, call('read_cards', { ids: 'a' }), ctx()).summary, 'Couldn’t read cards')
    const fine = agent.runCanvasTool(d, call('update_cards', { cards: [{ id: 'a', text: 'Use <kbd>Esc</kbd> to leave' }] }), ctx())
    assert.equal(fine.doc.cards[0].text, 'Use <kbd>Esc</kbd> to leave', 'inline tags in Markdown are fine')
    const html = agent.runCanvasTool(d, call('update_cards', { cards: [{ id: 'h', text: '<!doctype html><p>new</p>' }] }), ctx())
    assert.equal(html.doc.cards[1].text, '<!doctype html><p>new</p>', 'an HTML card takes a page')
  })

  test('the agent is told to build visuals beside the originals', () => {
    assert.match(agent.CANVAS_AGENT_PROMPT, /new HTML card/)
    assert.match(agent.CANVAS_AGENT_PROMPT, /keep the originals/)
  })

  test('limits hold: at most 30 new cards, sizes clamped, text capped', () => {
    const many = Array.from({ length: 40 }, (_, i) => ({ text: `c${i}`, w: 5 }))
    const out = agent.runCanvasTool(doc(), call('create_cards', { cards: many }), ctx())
    assert.equal(out.doc.cards.length, 30)
    assert.equal(out.doc.cards[0].w, canvas.MIN_CARD_WIDTH)
  })
})

describe('colours', () => {
  test('the agent can tint cards and frames from the palette, and nothing else', () => {
    let out = agent.runCanvasTool(
      doc(),
      call('create_cards', { cards: [{ text: 'a', color: 'green' }, { text: 'b', color: 'plaid' }, { kind: 'html', text: '<p>v</p>', color: 'blue' }] }),
      ctx(),
    )
    const [a, b, v] = out.doc.cards
    assert.deepEqual([a.color, 'color' in b, v.color], ['green', false, 'blue'])

    out = agent.runCanvasTool(out.doc, call('update_cards', { cards: [{ id: a.id, color: 'red' }, { id: v.id, color: 'none' }] }), ctx())
    assert.equal(out.doc.cards[0].color, 'red')
    assert.equal('color' in out.doc.cards[2], false, 'none takes the tint off')

    out = agent.runCanvasTool(out.doc, call('frame_cards', { ids: [a.id], title: 'Hot', color: 'orange' }), ctx())
    assert.equal(out.doc.frames[0].color, 'orange')

    const read = JSON.parse(agent.runCanvasTool(out.doc, call('read_canvas', {}), ctx()).result)
    assert.equal(read.cards.find((c) => c.id === a.id).color, 'red')
    assert.equal('color' in read.cards.find((c) => c.id === b.id), false)
    assert.equal(read.frames[0].color, 'orange')
  })

  test('the agent is told to use colour sparingly and to mean something by it', () => {
    assert.match(agent.CANVAS_AGENT_PROMPT, /Colour cards and frames only to group or mark/)
  })
})

describe('arrows the agent draws', () => {
  const two = () => doc([note('a', 0, 0, 'Cause'), note('b', 400, 0, 'Effect'), note('c', 800, 0, 'Other')])

  test('take a tidy label, heads and a line, and nothing off the list', () => {
    const out = agent.runCanvasTool(
      two(),
      call('connect_cards', {
        links: [
          { from: 'a', to: 'b', label: '  causes ', heads: 'both', shape: 'straight' },
          { from: 'b', to: 'c', heads: 'sideways', shape: 'dotted' },
        ],
      }),
      ctx(),
    )
    const [ab, bc] = out.doc.edges
    assert.deepEqual([ab.label, ab.heads, ab.shape], ['causes', 'both', 'straight'])
    assert.deepEqual([bc.label, bc.heads, bc.shape], [undefined, undefined, undefined])
    assert.equal(out.summary, 'Drew 2 arrows')
    assert.deepEqual(out.details, ['Cause to Effect, “causes”', 'Effect to Other'])
  })

  test('joining cards already joined restyles their arrow instead of skipping it', () => {
    const joined = agent.runCanvasTool(two(), call('connect_cards', { links: [{ from: 'a', to: 'b' }] }), ctx()).doc
    const out = agent.runCanvasTool(joined, call('connect_cards', { links: [{ from: 'a', to: 'b', label: 'leads to', heads: 'none' }] }), ctx())
    assert.equal(out.doc.edges.length, 1)
    assert.deepEqual([out.doc.edges[0].label, out.doc.edges[0].heads], ['leads to', 'none'])
    assert.equal(out.summary, 'Restyled an arrow')
    assert.deepEqual(JSON.parse(out.result), { connected: 0, restyled: 1, skipped: [] })
    const again = agent.runCanvasTool(out.doc, call('connect_cards', { links: [{ from: 'a', to: 'b' }] }), ctx())
    assert.equal(again.doc, out.doc, 'nothing new to say about it')
    assert.equal(JSON.parse(again.result).skipped.length, 1)
  })

  test('the canvas read back shows each arrow’s label and look', () => {
    const d = doc([note('a', 0, 0), note('b', 400, 0)], {
      edges: [{ id: 'e', from: 'a', to: 'b', label: 'then', heads: 'both', shape: 'straight' }],
    })
    const read = JSON.parse(agent.runCanvasTool(d, call('read_canvas', {}), ctx()).result)
    assert.deepEqual(read.arrows, [{ from: 'a', to: 'b', label: 'then', heads: 'both', shape: 'straight' }])
  })

  test('the agent is told when to label arrows', () => {
    assert.match(agent.CANVAS_AGENT_PROMPT, /Label an arrow/)
  })
})

describe('sizes and space', () => {
  /** Notes as tall as their lines, the way the page measures them. */
  const lines = ctx({ heightOf: (c) => 24 + 20 * c.text.split('\n').length })
  const boxesOf = (d) => d.cards.map((c) => ({ id: c.id, x: c.x, y: c.y, w: c.w, h: c.kind === 'html' ? c.h : lines.heightOf(c) }))
  const clearOfEachOther = (d) => {
    const boxes = boxesOf(d)
    for (let i = 0; i < boxes.length; i++)
      for (let j = i + 1; j < boxes.length; j++)
        assert.ok(!overlaps(boxes[i], boxes[j]), `${boxes[i].id} overlaps ${boxes[j].id}`)
  }
  const tenLines = Array.from({ length: 10 }, (_, i) => `line ${i}`).join('\n')

  test('without a page to measure in, a note’s height is estimated from its lines', () => {
    const one = agent.estimateHeight(note('a', 0, 0, 'one'))
    assert.ok(agent.estimateHeight(note('a', 0, 0, 'one\ntwo\nthree')) > one)
    assert.equal(agent.estimateHeight({ ...canvas.newHtmlCard(0, 0, 'h'), h: 250 }), 250)
  })

  test('new cards report their real size and place', () => {
    const out = agent.runCanvasTool(
      doc(),
      call('create_cards', { cards: [{ text: tenLines }, { kind: 'html', text: '<p>x</p>', w: 600, h: 240 }] }),
      lines,
    )
    const created = JSON.parse(out.result).created
    assert.deepEqual(created.map((c) => [c.w, c.h]), [[280, 224], [600, 240]])
    assert.deepEqual(created.map((c) => [c.x, c.y]), out.doc.cards.map((c) => [c.x, c.y]))
    clearOfEachOther(out.doc)
  })

  test('a group laid out with coordinates keeps its rows and columns at real sizes', () => {
    const grid = [
      { text: tenLines, x: 0, y: 0 },
      { text: 'b', x: 320, y: 0 },
      { text: 'c', x: 0, y: 100 },
      { text: 'd', x: 320, y: 100 },
    ]
    const out = agent.runCanvasTool(doc(), call('create_cards', { cards: grid }), lines)
    const [a, b, c, d] = out.doc.cards
    clearOfEachOther(out.doc)
    assert.deepEqual([a.y, b.y], [0, 0])
    assert.equal(c.y, d.y, 'the second row stays a row')
    assert.equal(c.y, 224 + 40, 'just below the tall note')
    assert.deepEqual([a.x, c.x, b.x, d.x], [0, 0, 320, 320])
    assert.deepEqual(JSON.parse(out.result).created.map((r) => r.moved ?? false), [false, false, true, true])
  })

  test('a group that would cover cards moves clear as a whole', () => {
    const d = doc([note('mine', 100, 100, 'keep me')])
    const out = agent.runCanvasTool(d, call('create_cards', { cards: [{ text: 'a', x: 0, y: 80 }, { text: 'b', x: 320, y: 80 }] }), lines)
    const [mine, a, b] = out.doc.cards
    assert.deepEqual([mine.x, mine.y], [100, 100], 'the user’s card stays')
    assert.equal(b.x - a.x, 320, 'the group keeps its shape')
    assert.equal(a.y, b.y)
    clearOfEachOther(out.doc)
  })

  test('a note that grows pushes the cards below it down, and its frame grows', () => {
    const d = doc([note('top', 0, 0, 'short'), note('below', 0, 100, 'next'), note('beside', 400, 100, 'side')], {
      frames: [{ id: 'f', x: -32, y: -32, w: 344, h: 240, title: 'Column' }],
    })
    const out = agent.runCanvasTool(d, call('update_cards', { cards: [{ id: 'top', text: tenLines }] }), lines)
    const [top, below, beside] = out.doc.cards
    assert.equal(top.y, 0)
    assert.equal(below.y, 224 + 40)
    assert.deepEqual([beside.x, beside.y], [400, 100], 'cards beside it stay')
    assert.equal('h' in below, false, 'a pushed note keeps following its text')
    assert.ok(out.doc.frames[0].y + out.doc.frames[0].h >= below.y + 44 + 32, 'the frame still holds the pushed card')
    assert.deepEqual(JSON.parse(out.result).moved, [{ id: 'below', x: 0, y: 264 }])
    assert.equal(out.summary, 'Edited a note and moved a card to make room')
    clearOfEachOther(out.doc)
  })

  test('a card that grows over one beside it slides clear instead, and pushes nothing', () => {
    const d = doc([note('a', 0, 100, 'a'), note('side', 400, 60, 'side'), note('below', 0, 200, 'below')])
    const out = agent.runCanvasTool(d, call('update_cards', { cards: [{ id: 'a', w: 600, text: tenLines }] }), lines)
    const [a, side, below] = out.doc.cards
    assert.deepEqual([side.x, side.y, below.x, below.y], [400, 60, 0, 200], 'the others stay where they were')
    assert.deepEqual(JSON.parse(out.result).moved, [{ id: 'a', x: a.x, y: a.y }])
    assert.equal(out.summary, 'Edited a note')
    clearOfEachOther(out.doc)
  })

  test('a card moved onto another slides to the nearest clear spot', () => {
    const d = doc([note('a', 0, 0, 'a'), note('b', 600, 0, 'b')])
    const out = agent.runCanvasTool(d, call('update_cards', { cards: [{ id: 'b', x: 20, y: 10 }] }), lines)
    const [a, b] = out.doc.cards
    assert.deepEqual([a.x, a.y], [0, 0])
    assert.ok(Math.hypot(b.x - 20, b.y - 10) < 400, `${b.x},${b.y} is near where it was asked`)
    assert.deepEqual(JSON.parse(out.result).moved, [{ id: 'b', x: b.x, y: b.y }])
    clearOfEachOther(out.doc)
  })
})

describe('the agent cannot overrun the document', () => {
  test('it stops adding cards at the canvas limit', () => {
    const full = doc(Array.from({ length: canvas.MAX_CARDS - 1 }, (_, i) => note(`k${i}`, (i % 70) * 400, Math.floor(i / 70) * 200)))
    const out = agent.runCanvasTool(full, call('create_cards', { cards: [{ text: 'a', x: -9000, y: 0 }, { text: 'b' }, { text: 'c' }] }), ctx())
    assert.equal(out.doc.cards.length, canvas.MAX_CARDS)
    assert.equal(JSON.parse(out.result).skipped, 2)
  })

  test('one call may add only so much text', () => {
    const big = 'x'.repeat(400_000)
    const out = agent.runCanvasTool(doc(), call('create_cards', { cards: [1, 2, 3].map(() => ({ kind: 'html', text: big })) }), ctx())
    assert.equal(out.doc.cards.length, 2)
    assert.equal(JSON.parse(out.result).skipped, 1)
  })

  test('far-flung coordinates are pulled back onto the canvas', () => {
    const d = doc([note('a', 0, 0)])
    let out = agent.runCanvasTool(d, call('create_cards', { cards: [{ text: 'far', x: 1e15, y: -1e15 }] }), ctx())
    assert.deepEqual([out.doc.cards[1].x, out.doc.cards[1].y], [canvas.MAX_COORD, -canvas.MAX_COORD])
    out = agent.runCanvasTool(d, call('update_cards', { cards: [{ id: 'a', x: -1e15 }] }), ctx())
    assert.equal(out.doc.cards[0].x, -canvas.MAX_COORD)
  })
})

describe('each turn', () => {
  test('carries the selected cards in full, and says what is visible', () => {
    const d = doc([note('a', 0, 0, 'picked text'), note('b', 400, 0, 'other')])
    const context = agent.turnContext(d, ctx({ selection: ['a'] }))
    assert.match(context, /picked text/)
    assert.doesNotMatch(context, /other/)
    assert.match(context, /2 cards/)
  })
})

describe('a turn', () => {
  const scripted = (rounds) => {
    const sent = []
    const chat = async (request, onEvent) => {
      sent.push(structuredClone(request))
      for (const event of rounds.shift() ?? [{ type: 'done' }]) onEvent(event)
      return null
    }
    return { sent, chat }
  }
  const deps = (chat, over = {}) => {
    let current = doc([note('a', 0, 0)])
    const applied = []
    const log = []
    return {
      applied,
      log,
      current: () => current,
      deps: {
        chat,
        newRequestId: () => `r${applied.length}-${log.length}`,
        getDoc: () => current,
        apply: (next) => {
          applied.push(next)
          current = next
        },
        context: () => ctx(),
        report: (entry) => log.push(entry),
        cancelled: () => false,
        ...over,
      },
    }
  }

  test('runs tools, feeds their results back, and ends on the reply', async () => {
    const { sent, chat } = scripted([
      [
        { type: 'delta', text: 'On it.' },
        { type: 'tool_calls', calls: [{ id: 't1', name: 'create_cards', arguments: '{"cards":[{"text":"hi"}]}' }] },
        { type: 'done' },
      ],
      [{ type: 'delta', text: 'Added a card.' }, { type: 'done' }],
    ])
    const run = deps(chat)
    const out = await agent.runTurn([], 'add a card', run.deps)
    assert.equal(out.reply, 'Added a card.')
    assert.equal(out.created.length, 1)
    assert.equal(run.applied.length, 1)
    assert.equal(run.current().cards.length, 2)
    assert.equal(sent[0].messages[0].role, 'system')
    assert.match(sent[0].messages.at(-1).content, /add a card/)
    const [assistant, tool] = sent[1].messages.slice(-2)
    assert.deepEqual([assistant.role, assistant.tool_calls[0].id, tool.role, tool.tool_call_id], ['assistant', 't1', 'tool', 't1'])
    assert.ok(run.log.some((e) => e.type === 'step' && e.text === 'Added a note'))
  })

  test('each round opens with its thinking, streamed as it comes', async () => {
    const { chat } = scripted([
      [{ type: 'reasoning', text: 'Plan the ' }, { type: 'reasoning', text: 'cards.' }, { type: 'delta', text: 'Done.' }, { type: 'done' }],
    ])
    const run = deps(chat)
    await agent.runTurn([], 'go', run.deps)
    const kinds = run.log.map((e) => e.type)
    assert.equal(kinds[0], 'round')
    assert.deepEqual(run.log.filter((e) => e.type === 'think').map((e) => e.text), ['Plan the ', 'Plan the cards.'])
    assert.ok(kinds.indexOf('think') < kinds.indexOf('reply'))
  })

  test('steps carry their card titles to the log', async () => {
    const { chat } = scripted([
      [{ type: 'tool_calls', calls: [{ id: 't1', name: 'create_cards', arguments: '{"cards":[{"text":"Idea"}]}' }] }, { type: 'done' }],
      [{ type: 'delta', text: 'ok' }, { type: 'done' }],
    ])
    const run = deps(chat)
    await agent.runTurn([], 'go', run.deps)
    assert.deepEqual(run.log.find((e) => e.type === 'step').details, ['Idea'])
    assert.equal(run.log.filter((e) => e.type === 'round').length, 2)
  })

  test('attached files travel with the request, fenced', async () => {
    const { sent, chat } = scripted([[{ type: 'delta', text: 'ok' }, { type: 'done' }]])
    await agent.runTurn([], 'summarise this', deps(chat).deps, [{ name: 'notes.md', text: 'alpha' }])
    const content = sent[0].messages.at(-1).content
    assert.match(content, /<attachment name="notes.md">\nalpha\n<\/attachment>/)
    assert.ok(content.indexOf('<attachment') < content.indexOf('summarise this'))
  })

  test('the web tools search, then read only pages it found or was given', async () => {
    const calls = (list) => ({ type: 'tool_calls', calls: list.map(([name, args], i) => ({ id: `w${i}`, name, arguments: JSON.stringify(args) })) })
    const { sent, chat } = scripted([
      [calls([['web_search', { query: 'kyoto weather' }]]), { type: 'done' }],
      [
        calls([
          ['web_fetch', { url: 'https://weather.example/kyoto' }],
          ['web_fetch', { url: 'https://evil.example/' }],
          ['web_search', { query: 'Kyoto weather' }],
          ['web_fetch', { url: 'https://given.example/page' }],
        ]),
        { type: 'done' },
      ],
      [{ type: 'delta', text: 'ok' }, { type: 'done' }],
    ])
    const searched = []
    const fetched = []
    const run = deps(chat, {
      web: {
        search: async (query) => {
          searched.push(query)
          return [{ title: 'Kyoto forecast', url: 'https://weather.example/kyoto', snippet: 'Mild' }]
        },
        fetch: async (url) => {
          fetched.push(url)
          return { url, title: 'Forecast', text: 'Sunny all week' }
        },
      },
    })
    await agent.runTurn([], 'check https://given.example/page and the weather', run.deps)
    assert.deepEqual(searched, ['kyoto weather'])
    assert.deepEqual(fetched, ['https://weather.example/kyoto', 'https://given.example/page'])
    const results = sent[2].messages.filter((m) => m.role === 'tool').map((m) => m.content)
    assert.match(results[0], /Kyoto forecast/)
    assert.match(results[1], /Sunny all week/)
    assert.match(results[2], /^Error: .*web_search/)
    assert.match(results[3], /^Error: .*already searched/)
    const steps = run.log.filter((e) => e.type === 'step')
    assert.equal(steps[0].text, 'Searched the web for “kyoto weather”')
    assert.deepEqual(steps[0].details, ['Kyoto forecast'])
    assert.equal(steps[1].text, 'Read a page')
    assert.deepEqual(steps[1].details, ['Forecast (weather.example)'])
  })

  test('without web access the web tools refuse', async () => {
    const { sent, chat } = scripted([
      [{ type: 'tool_calls', calls: [{ id: 'w', name: 'web_search', arguments: '{"query":"x"}' }] }, { type: 'done' }],
      [{ type: 'delta', text: 'ok' }, { type: 'done' }],
    ])
    await agent.runTurn([], 'go', deps(chat).deps)
    assert.match(sent[1].messages.at(-1).content, /^Error: web access is off/)
  })

  test('a failed search says so', async () => {
    const { chat } = scripted([
      [{ type: 'tool_calls', calls: [{ id: 'w', name: 'web_search', arguments: '{"query":"x"}' }] }, { type: 'done' }],
      [{ type: 'delta', text: 'ok' }, { type: 'done' }],
    ])
    const run = deps(chat, { web: { search: async () => { throw new Error('offline') }, fetch: async () => ({}) } })
    await agent.runTurn([], 'go', run.deps)
    assert.equal(run.log.find((e) => e.type === 'step').text, 'Couldn’t search the web')
  })

  test('a provider error ends the turn with it, and nothing changes', async () => {
    const { chat } = scripted([[{ type: 'error', message: 'provider error 401' }]])
    const run = deps(chat)
    const out = await agent.runTurn([], 'hello', run.deps)
    assert.equal(out.error, 'provider error 401')
    assert.equal(run.applied.length, 0)
  })

  test('a rejected request is reported as an error', async () => {
    const run = deps(async () => {
      throw new Error('no AI model configured')
    })
    assert.equal((await agent.runTurn([], 'hello', run.deps)).error, 'no AI model configured')
  })

  test('stopping drops the pending tool calls', async () => {
    const { chat } = scripted([
      [{ type: 'tool_calls', calls: [{ id: 't1', name: 'delete_cards', arguments: '{"ids":["a"]}' }] }, { type: 'done' }],
    ])
    const run = deps(chat, { cancelled: () => true })
    const out = await agent.runTurn([], 'clear it', run.deps)
    assert.equal(out.stopped, true)
    assert.equal(run.current().cards.length, 1)
  })

  test('a round runs only so many calls, but every call gets an answer', async () => {
    const many = Array.from({ length: agent.MAX_CALLS + 4 }, (_, i) => ({ id: `t${i}`, name: 'read_canvas', arguments: '{}' }))
    const { sent, chat } = scripted([[{ type: 'tool_calls', calls: many }, { type: 'done' }], [{ type: 'delta', text: 'ok' }, { type: 'done' }]])
    await agent.runTurn([], 'go', deps(chat).deps)
    const results = sent[1].messages.filter((m) => m.role === 'tool')
    assert.equal(results.length, agent.MAX_CALLS + 4)
    assert.equal(results.filter((m) => m.content.startsWith('Error: ')).length, 4)
  })

  test('the last allowed round withholds tools so the agent must answer', async () => {
    const calls = { type: 'tool_calls', calls: [{ id: 't', name: 'read_canvas', arguments: '{}' }] }
    const { sent, chat } = scripted(Array.from({ length: agent.MAX_ROUNDS }, () => [calls, { type: 'done' }]))
    const out = await agent.runTurn([], 'loop', deps(chat).deps)
    assert.equal(sent.length, agent.MAX_ROUNDS)
    assert.equal(sent.at(-1).no_tools, true)
    assert.equal(sent[0].no_tools, false)
    assert.ok(out.error || out.reply === '')
  })
})

describe('the pill', () => {
  test('shows a reply as one plain line', () => {
    assert.equal(agent.plainLine('I added **two cards** and `one` [link](https://x.y) _here_:\nmore'), 'I added two cards and one link here:')
    assert.equal(agent.plainLine('## Done\n'), 'Done')
    assert.equal(agent.plainLine('\n\n- first item'), 'first item')
  })
})
