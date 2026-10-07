// `page_menu.rs` PAGE_MENU_JS: posts what was right-clicked to `wisPageMenu` and paints the selection while the menu is up.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'

const FALLBACK = 'background-color:rgba(179, 215, 255, 0.6)'
const PREFERRED = 'background-color:light-dark(rgba(179, 215, 255, 0.6), rgba(63, 99, 139, 0.8))'
// As `page_script()` bakes in the selection colours.
const SCRIPT = rawStringConst('page_menu.rs', 'PAGE_MENU_JS')
  .replaceAll('__WIS_SEL_FALLBACK__', FALLBACK)
  .replaceAll('__WIS_SEL_PREFERRED__', PREFERRED)

const STYLE_ID = 'wis-menu-selection-style'
const HIGHLIGHT = 'wis-menu-selection'
const PAINT =
  '::selection{background-color:transparent}' +
  `::highlight(${HIGHLIGHT}){${FALLBACK}}` +
  `::highlight(${HIGHLIGHT}){${PREFERRED}}`
const CLEARED = `::highlight(${HIGHLIGHT}){background-color:transparent}`

/** An element; `attrs` is what `getAttribute` returns. */
function el(tag, props = {}, attrs = {}) {
  return { tagName: tag.toUpperCase(), ...props, getAttribute: (n) => attrs[n] ?? null }
}

function range(collapsed = false) {
  const r = { collapsed }
  r.cloneRange = () => ({ collapsed, clonedFrom: r })
  return r
}

/** A selection of `text` over `ranges`. */
function selectionOf(text, ranges = text.trim() ? [range()] : []) {
  return { rangeCount: ranges.length, getRangeAt: (i) => ranges[i], toString: () => text }
}

class FakeHighlight {
  constructor() {
    this.ranges = []
  }
  add(r) {
    this.ranges.push(r)
  }
  get size() {
    return this.ranges.length
  }
}

/** A fresh top-level page running the script. */
function loadPage({ top = true, highlights = true, raf = true, weakRef = true, handler = true, head = true } = {}) {
  const sent = []
  const listeners = []
  let frames = []
  const container = () => {
    const children = []
    return { children, appendChild: (n) => (children.push(n), n) }
  }
  const document = { documentElement: container(), head: head ? container() : null }
  const appended = () => [...(document.head?.children ?? []), ...document.documentElement.children]
  document.getElementById = (id) => appended().find((n) => n.id === id) ?? null
  document.createElement = (tag) => ({ tagName: tag.toUpperCase(), id: '', textContent: '' })

  const registry = new Map()
  const CSS = highlights ? { highlights: registry } : {}
  const window = {
    innerWidth: 1280,
    innerHeight: 720,
    CSS,
    addEventListener: (type, fn, capture) => listeners.push({ type, fn, capture }),
    getSelection: () => p.selection,
  }
  window.top = top ? window : {}
  window.self = window
  if (highlights) window.Highlight = FakeHighlight
  if (raf) window.requestAnimationFrame = (fn) => frames.push(fn)
  if (handler) window.webkit = { messageHandlers: { wisPageMenu: { postMessage: (body) => sent.push(body) } } }

  const run = () =>
    new Function('window', 'document', 'CSS', 'Highlight', 'requestAnimationFrame', 'WeakRef', SCRIPT)(
      window,
      document,
      CSS,
      window.Highlight,
      window.requestAnimationFrame,
      weakRef ? WeakRef : undefined,
    )
  run()

  const fire = (type, event) => listeners.filter((l) => l.type === type).forEach((l) => l.fn(event))
  const p = {
    sent,
    window,
    document,
    registry,
    listeners,
    selection: selectionOf(''),
    reinject: run,
    posts: () => sent.map((body) => JSON.parse(body)),
    style: () => document.getElementById(STYLE_ID),
    frame() {
      const due = frames
      frames = []
      due.forEach((fn) => fn())
    },
    /** Right-clicks the innermost of `path`; returns the event. */
    rightClick(path, extra = {}) {
      const e = {
        clientX: 40,
        clientY: 50,
        defaultPrevented: false,
        target: path[0],
        composedPath: () => [...path, document, window],
        preventDefault: () => (e.defaultPrevented = true),
        ...extra,
      }
      fire('contextmenu', e)
      return e
    },
    press(type, button) {
      const e = { button, prevented: false, preventDefault: () => (e.prevented = true) }
      fire(type, e)
      return e
    },
  }
  return p
}

const EMPTY = {
  linkHref: null,
  linkText: null,
  imageSrc: null,
  mediaKind: null,
  mediaSrc: null,
  selection: null,
}

describe('installation', () => {
  test('an iframe keeps the engine menu: nothing is installed', () => {
    const p = loadPage({ top: false })
    assert.equal(p.listeners.length, 0)
    assert.equal(p.window.__wisPageMenu, undefined)
    assert.equal(p.window.__wisMenuClearPaint, undefined)
  })

  test('a second injection installs nothing more', () => {
    const p = loadPage()
    const count = p.listeners.length
    p.reinject()
    assert.equal(p.listeners.length, count)
    p.rightClick([el('p')])
    assert.equal(p.sent.length, 1)
  })

  test('the menu listener runs at bubble phase so a page handler goes first; the press listeners at capture', () => {
    const p = loadPage()
    const phase = Object.fromEntries(p.listeners.map((l) => [l.type, l.capture]))
    assert.deepEqual(phase, { contextmenu: false, mousedown: true, pointerdown: true })
  })
})

describe('what a right-click posts', () => {
  test('a plain right-click is taken from the engine and posts the pointer and view size as a JSON string', () => {
    const p = loadPage()
    const e = p.rightClick([el('p')])
    assert.equal(e.defaultPrevented, true)
    assert.equal(typeof p.sent[0], 'string', 'every platform parses the body as a JSON string')
    // Exactly the camelCase keys `dispatch_message` reads.
    assert.deepEqual(p.posts(), [{ x: 40, y: 50, viewWidth: 1280, viewHeight: 720, ...EMPTY }])
  })

  test('a link posts its href and trimmed text', () => {
    const p = loadPage()
    p.rightClick([el('span'), el('a', { href: 'https://example.com/', textContent: '  Example  ' })])
    assert.equal(p.posts()[0].linkHref, 'https://example.com/')
    assert.equal(p.posts()[0].linkText, 'Example')
  })

  test('an image inside a link reports both', () => {
    const p = loadPage()
    p.rightClick([el('img', { currentSrc: 'https://example.com/a.png' }), el('a', { href: 'https://example.com/' })])
    assert.equal(p.posts()[0].imageSrc, 'https://example.com/a.png')
    assert.equal(p.posts()[0].linkHref, 'https://example.com/')
  })

  test('an image with no current source is not reported', () => {
    const p = loadPage()
    p.rightClick([el('img', { currentSrc: '' })])
    assert.equal(p.posts()[0].imageSrc, null)
  })

  test('the innermost link with an href wins', () => {
    const p = loadPage()
    p.rightClick([
      el('a', { href: '', textContent: 'anchor' }),
      el('a', { href: 'https://inner.example/', textContent: 'inner' }),
      el('a', { href: 'https://outer.example/', textContent: 'outer' }),
    ])
    assert.equal(p.posts()[0].linkHref, 'https://inner.example/')
    assert.equal(p.posts()[0].linkText, 'inner')
  })

  test('a video reports its kind and current source, an audio element its src when no current source', () => {
    const p = loadPage()
    p.rightClick([el('video', { currentSrc: 'https://example.com/v.mp4', src: 'ignored' })])
    p.rightClick([el('audio', { currentSrc: '', src: 'https://example.com/a.mp3' })])
    p.rightClick([el('video', { currentSrc: '', src: '' })])
    assert.deepEqual(
      p.posts().map((b) => [b.mediaKind, b.mediaSrc]),
      [
        ['video', 'https://example.com/v.mp4'],
        ['audio', 'https://example.com/a.mp3'],
        ['video', null],
      ],
    )
  })

  test('blank or missing link text is null', () => {
    const p = loadPage()
    p.rightClick([el('a', { href: 'https://example.com/', textContent: '   ' })])
    p.rightClick([el('a', { href: 'https://example.com/', textContent: null })])
    assert.deepEqual(
      p.posts().map((b) => b.linkText),
      [null, null],
    )
  })

  test('link text is capped at 400 characters without splitting a surrogate pair', () => {
    const p = loadPage()
    p.rightClick([el('a', { href: 'https://example.com/', textContent: 'b'.repeat(500) })])
    p.rightClick([el('a', { href: 'https://example.com/', textContent: 'a'.repeat(399) + '\u{1F600}tail' })])
    const [long, astral] = p.posts().map((b) => b.linkText)
    assert.equal(long.length, 400)
    assert.equal(astral, 'a'.repeat(399))
  })

  test('path entries without a tag name are passed over', () => {
    const p = loadPage()
    p.rightClick([{ nodeType: 3 }, el('a', { href: 'https://example.com/' })])
    assert.equal(p.posts()[0].linkHref, 'https://example.com/')
  })

  test('a composedPath that throws or comes back empty falls back to the event target', () => {
    const p = loadPage()
    const link = el('a', { href: 'https://example.com/' })
    p.rightClick([link], {
      composedPath: () => {
        throw new Error('nope')
      },
    })
    p.rightClick([link], { composedPath: () => [] })
    assert.deepEqual(
      p.posts().map((b) => b.linkHref),
      ['https://example.com/', 'https://example.com/'],
    )
  })

  test('a right-click with neither a path nor a target still opens a plain menu', () => {
    const p = loadPage()
    p.rightClick([undefined], { composedPath: () => null })
    assert.deepEqual(p.posts(), [{ x: 40, y: 50, viewWidth: 1280, viewHeight: 720, ...EMPTY }])
    assert.equal(p.window.__wisInspectTarget, undefined)
  })

  test('a right-click the page already handled is left alone', () => {
    const p = loadPage()
    let prevented = 0
    p.rightClick([el('p')], { defaultPrevented: true, preventDefault: () => prevented++ })
    assert.equal(prevented, 0)
    assert.deepEqual(p.sent, [])
  })

  test('a missing or throwing handler is tolerated', () => {
    const missing = loadPage({ handler: false })
    assert.doesNotThrow(() => missing.rightClick([el('p')]))
    const throwing = loadPage()
    throwing.window.webkit.messageHandlers.wisPageMenu.postMessage = () => {
      throw new Error('gone')
    }
    assert.doesNotThrow(() => throwing.rightClick([el('p')]))
  })
})

describe('editable targets keep the engine menu', () => {
  const editable = [
    ['a textarea', el('textarea')],
    ['an input with no type', el('input')],
    ['an input with an empty type', el('input', {}, { type: '' })],
    ['an input typed TEXT in capitals', el('input', {}, { type: 'TEXT' })],
    ...['search', 'url', 'email', 'tel', 'password', 'number'].map((t) => [`a ${t} input`, el('input', {}, { type: t })]),
    ['a contenteditable element', el('div', { isContentEditable: true })],
  ]
  for (const [what, target] of editable) {
    test(`${what} is not taken`, () => {
      const p = loadPage()
      const e = p.rightClick([target])
      assert.equal(e.defaultPrevented, false)
      assert.deepEqual(p.sent, [])
    })
  }

  test('a link inside an editable region is not taken either', () => {
    const p = loadPage()
    p.rightClick([el('a', { href: 'https://example.com/' }), el('div', { isContentEditable: true })])
    assert.deepEqual(p.sent, [])
  })

  for (const type of ['checkbox', 'button', 'submit', 'range', 'file']) {
    test(`a ${type} input is not editable, so Wis's menu opens`, () => {
      const p = loadPage()
      p.rightClick([el('input', {}, { type })])
      assert.equal(p.sent.length, 1)
    })
  }
})

describe('the selection', () => {
  test('a selection is posted trimmed', () => {
    const p = loadPage()
    p.selection = selectionOf('  hello world  ')
    p.rightClick([el('p')])
    assert.equal(p.posts()[0].selection, 'hello world')
  })

  test('a whitespace-only selection is null and paints nothing', () => {
    const p = loadPage()
    p.selection = selectionOf('   ', [range()])
    p.rightClick([el('p')])
    assert.equal(p.posts()[0].selection, null)
    assert.equal(p.registry.has(HIGHLIGHT), false)
  })

  test('a selection is capped at 32000 characters', () => {
    const p = loadPage()
    p.selection = selectionOf('x'.repeat(32001))
    p.rightClick([el('p')])
    assert.equal(p.posts()[0].selection.length, 32000)
  })

  test('a getSelection that throws still opens the menu, with no selection', () => {
    const p = loadPage()
    p.window.getSelection = () => {
      throw new Error('detached')
    }
    p.rightClick([el('p')])
    assert.equal(p.posts()[0].selection, null)
  })

  test("the selection is repainted as a highlight in Wis's colours while the menu is up", () => {
    const p = loadPage()
    const r = range()
    p.selection = selectionOf('hello', [r])
    p.rightClick([el('p')])
    assert.equal(p.style().textContent, PAINT)
    assert.equal(p.document.head.children[0], p.style())
    const painted = p.registry.get(HIGHLIGHT)
    assert.equal(painted.size, 1)
    assert.notEqual(painted.ranges[0], r, 'a clone, so the page moving its selection does not move the paint')
  })

  test('collapsed ranges are left out, and only collapsed ranges paint nothing', () => {
    const p = loadPage()
    p.selection = selectionOf('hello', [range(true), range()])
    p.rightClick([el('p')])
    assert.equal(p.registry.get(HIGHLIGHT).size, 1)

    const q = loadPage()
    q.selection = selectionOf('hello', [range(true)])
    q.rightClick([el('p')])
    assert.equal(q.registry.has(HIGHLIGHT), false)
    assert.equal(q.style(), null)
  })

  test('without the CSS Custom Highlight API nothing is painted but the menu still opens', () => {
    const p = loadPage({ highlights: false })
    p.selection = selectionOf('hello')
    p.rightClick([el('p')])
    assert.equal(p.style(), null)
    assert.equal(p.posts()[0].selection, 'hello')
  })

  test('with no head the style goes on the root element', () => {
    const p = loadPage({ head: false })
    p.selection = selectionOf('hello')
    p.rightClick([el('p')])
    assert.equal(p.document.documentElement.children[0].id, STYLE_ID)
  })

  test('a later paint reuses the style node', () => {
    const p = loadPage()
    p.selection = selectionOf('hello')
    p.rightClick([el('p')])
    p.rightClick([el('p')])
    assert.equal(p.document.head.children.length, 1)
  })

  test('a paint that throws still opens the menu', () => {
    const p = loadPage()
    p.registry.set = () => {
      throw new Error('no')
    }
    p.selection = selectionOf('hello')
    assert.doesNotThrow(() => p.rightClick([el('p')]))
    assert.equal(p.posts()[0].selection, 'hello')
  })
})

describe('clearing the paint', () => {
  /** A page whose menu is up over a painted selection. */
  function painted(options) {
    const p = loadPage(options)
    p.selection = selectionOf('hello')
    p.rightClick([el('p')])
    return p
  }

  test('the clear a right-click queues before painting does not drop the paint it precedes', () => {
    const p = painted()
    p.frame()
    assert.equal(p.registry.has(HIGHLIGHT), true)
  })

  test("the chrome's clear hides the paint at once and drops the highlight on the next frame", () => {
    const p = painted()
    p.window.__wisMenuClearPaint()
    assert.equal(p.style().textContent, CLEARED)
    assert.equal(p.registry.has(HIGHLIGHT), true)
    p.frame()
    assert.equal(p.registry.has(HIGHLIGHT), false)
  })

  test('a newer paint before that frame survives the older clear', () => {
    const p = painted()
    p.window.__wisMenuClearPaint()
    p.rightClick([el('p')])
    p.frame()
    assert.equal(p.registry.has(HIGHLIGHT), true)
    assert.equal(p.style().textContent, PAINT)
  })

  test('without requestAnimationFrame the highlight is dropped at once', () => {
    const p = painted({ raf: false })
    p.window.__wisMenuClearPaint()
    assert.equal(p.registry.has(HIGHLIGHT), false)
  })

  test('a clear before any paint creates no style node', () => {
    const p = loadPage()
    p.window.__wisMenuClearPaint()
    p.frame()
    assert.equal(p.style(), null)
  })

  test('a clear without the highlight API, or with a throwing delete, does not throw', () => {
    assert.doesNotThrow(() => loadPage({ highlights: false }).window.__wisMenuClearPaint())
    const p = painted()
    p.registry.delete = () => {
      throw new Error('no')
    }
    p.window.__wisMenuClearPaint()
    assert.doesNotThrow(() => p.frame())
    const q = loadPage()
    q.document.getElementById = () => {
      throw new Error('no')
    }
    assert.doesNotThrow(() => q.window.__wisMenuClearPaint())
  })

  test('a left press anywhere clears the paint, other buttons do not', () => {
    const p = painted()
    p.press('pointerdown', 2)
    assert.equal(p.style().textContent, PAINT)
    p.press('pointerdown', 0)
    assert.equal(p.style().textContent, CLEARED)
  })
})

describe('keeping the selection for the press that focuses this pane', () => {
  test('an armed left press over a selection is prevented, once', () => {
    const p = loadPage()
    p.selection = selectionOf('hello')
    p.window.__wisKeepSelectionOnce()
    assert.equal(p.press('mousedown', 0).prevented, true)
    assert.equal(p.press('mousedown', 0).prevented, false)
  })

  test('an unarmed press is never prevented', () => {
    const p = loadPage()
    p.selection = selectionOf('hello')
    assert.equal(p.press('mousedown', 0).prevented, false)
  })

  test('an armed press with another button is not prevented and disarms', () => {
    const p = loadPage()
    p.selection = selectionOf('hello')
    p.window.__wisKeepSelectionOnce()
    assert.equal(p.press('mousedown', 2).prevented, false)
    assert.equal(p.press('mousedown', 0).prevented, false)
  })

  test('an armed press over no selection or a caret is not prevented', () => {
    const p = loadPage()
    p.selection = selectionOf('')
    p.window.__wisKeepSelectionOnce()
    assert.equal(p.press('mousedown', 0).prevented, false)
    p.selection = selectionOf('', [range(true)])
    p.window.__wisKeepSelectionOnce()
    assert.equal(p.press('mousedown', 0).prevented, false)
  })

  test('a getSelection that throws is tolerated', () => {
    const p = loadPage()
    p.window.getSelection = () => {
      throw new Error('detached')
    }
    p.window.__wisKeepSelectionOnce()
    assert.doesNotThrow(() => p.press('mousedown', 0))
  })
})

describe('the node parked for Inspect Element', () => {
  test('the innermost right-clicked node is parked behind a WeakRef', () => {
    const p = loadPage()
    const target = el('span')
    p.rightClick([target, el('p')])
    assert.equal(p.window.__wisInspectTarget.deref(), target)
  })

  test('without WeakRef the node itself is parked', () => {
    const p = loadPage({ weakRef: false })
    const target = el('span')
    p.rightClick([target])
    assert.equal(p.window.__wisInspectTarget, target)
  })

  test('nothing is parked for a right-click left to the engine', () => {
    const p = loadPage()
    p.rightClick([el('textarea')])
    assert.equal(p.window.__wisInspectTarget, undefined)
  })
})
