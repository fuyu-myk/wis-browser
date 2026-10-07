// `page_source.rs` CAPTURE_JS: serialises a clone of the live DOM for View Source, without Wis's injected nodes.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'

const SCRIPT = rawStringConst('page_source.rs', 'CAPTURE_JS')
const MAX = 4000000

/** The ids of nodes Wis's own page scripts put into a document. */
const WIS_IDS = [
  'wis-cosmetic',
  'wis-cosmetic-host',
  'wis-find-style',
  'wis-menu-selection-style',
  '__wisHoverGuard',
  '__wisSwipeNav',
]

class Text {
  constructor(text) {
    this.text = text
    this.parentNode = null
  }
  cloneNode() {
    return new Text(this.text)
  }
  get outerHTML() {
    return this.text
  }
}

/** An element serialising like the DOM, with an id-list `querySelectorAll`. */
class El {
  constructor(tag, attrs = {}, children = []) {
    this.tagName = tag.toUpperCase()
    this.attrs = attrs
    this.id = attrs.id ?? ''
    this.parentNode = null
    this.childNodes = []
    children.forEach((c) => this.appendChild(c))
  }
  appendChild(c) {
    c.parentNode = this
    this.childNodes.push(c)
    return c
  }
  removeChild(c) {
    this.childNodes.splice(this.childNodes.indexOf(c), 1)
    c.parentNode = null
    return c
  }
  cloneNode(deep) {
    return new El(this.tagName, { ...this.attrs }, deep ? this.childNodes.map((c) => c.cloneNode(true)) : [])
  }
  *descendants() {
    for (const c of this.childNodes) {
      if (c instanceof El) {
        yield c
        yield* c.descendants()
      }
    }
  }
  querySelectorAll(selector) {
    const ids = selector.split(',').map((part) => {
      const s = part.trim()
      if (!s.startsWith('#')) throw new Error(`unexpected selector ${s}`)
      return s.slice(1)
    })
    return [...this.descendants()].filter((e) => ids.includes(e.id))
  }
  get outerHTML() {
    const tag = this.tagName.toLowerCase()
    const attrs = Object.entries(this.attrs)
      .map(([k, v]) => ` ${k}="${v}"`)
      .join('')
    return `<${tag}${attrs}>${this.childNodes.map((c) => c.outerHTML).join('')}</${tag}>`
  }
}

/** A page carrying every node Wis injects next to its own content. */
function pageWithWisNodes() {
  return new El('html', { lang: 'en' }, [
    new El('head', {}, [
      new El('title', {}, [new Text('Hi')]),
      new El('style', { id: 'wis-find-style' }),
      new El('style', { id: 'wis-menu-selection-style' }),
      new El('style', { id: 'wis-cosmetic' }),
      new El('style', { id: '__wisHoverGuard' }),
    ]),
    new El('body', { class: 'page' }, [
      new El('p', {}, [new Text('Hello')]),
      new El('div', { id: 'wis-cosmetic-host' }, [new El('style', { id: 'wis-cosmetic' })]),
      new El('div', { id: '__wisSwipeNav' }),
    ]),
  ])
}
const CLEAN = '<html lang="en"><head><title>Hi</title></head><body class="page"><p>Hello</p></body></html>'

/** A root whose clone serialises to `html` and matches nothing. */
const rootOf = (html) => ({ cloneNode: () => ({ querySelectorAll: () => [], outerHTML: html }) })

/**
 * Evaluates the script once, as `capture_page_source` does. `channel` is
 * `webkit` (macOS, and the shims elsewhere), `window` or `none`.
 */
function capture({ root = pageWithWisNodes(), doctype = null, top = true, channel = 'webkit' } = {}) {
  const sent = []
  const window = {}
  window.top = top ? window : {}
  window.self = window
  if (channel === 'webkit') window.webkit = { messageHandlers: { wisPageSource: { postMessage: (b) => sent.push(b) } } }
  if (channel === 'window') window.wisPageSource = { postMessage: (b) => sent.push(b) }
  const document = { documentElement: root, doctype }
  const run = () => new Function('window', 'document', SCRIPT)(window, document)
  run()
  return { sent, root, window, run }
}

describe('what is captured', () => {
  test('the serialised document is posted as the html and truncated fields the macOS handler reads', () => {
    const { sent } = capture()
    assert.deepEqual(sent, [{ html: CLEAN, truncated: false }])
  })

  test("Wis's injected nodes are dropped from the capture", () => {
    const [{ html }] = capture().sent
    for (const id of WIS_IDS) assert.ok(!html.includes(id), id)
  })

  test('the live page keeps its own copy of those nodes', () => {
    const { root } = capture()
    const ids = [...root.descendants()].map((e) => e.id).filter(Boolean)
    assert.deepEqual([...new Set(ids)].sort(), [...WIS_IDS].sort())
  })

  test('a matched node already detached is skipped', () => {
    const root = { cloneNode: () => ({ querySelectorAll: () => [{ parentNode: null }], outerHTML: '<html></html>' }) }
    assert.deepEqual(capture({ root }).sent, [{ html: '<html></html>', truncated: false }])
  })

  test('each evaluation answers afresh', () => {
    const p = capture()
    p.run()
    assert.equal(p.sent.length, 2)
  })

  test('an iframe answers nothing', () => {
    assert.deepEqual(capture({ top: false }).sent, [])
  })

  test("the video pop-out's hover button, style and overlay are dropped from the capture", () => {
    const root = new El('html', {}, [
      new El('head', {}, [new El('style', { id: '__wis-popout-style' })]),
      new El('body', {}, [
        new El('p', {}, [new Text('Hello')]),
        new El('div', { id: '__wis-popout-overlay' }, [new El('button', { id: '__wis-popout-x' })]),
      ]),
      new El('button', { id: '__wis-popout-hover' }),
    ])
    const [{ html }] = capture({ root }).sent
    assert.equal(html, '<html><head></head><body><p>Hello</p></body></html>')
  })
})

describe('the doctype', () => {
  const doctypes = [
    ['none adds no prologue', null, ''],
    ['an HTML5 doctype', { name: 'html', publicId: '', systemId: '' }, '<!DOCTYPE html>\n'],
    [
      'a public and a system id',
      {
        name: 'html',
        publicId: '-//W3C//DTD HTML 4.01//EN',
        systemId: 'http://www.w3.org/TR/html4/strict.dtd',
      },
      '<!DOCTYPE html PUBLIC "-//W3C//DTD HTML 4.01//EN" "http://www.w3.org/TR/html4/strict.dtd">\n',
    ],
    [
      'a public id alone',
      { name: 'html', publicId: '-//W3C//DTD HTML 4.01 Transitional//EN', systemId: '' },
      '<!DOCTYPE html PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN">\n',
    ],
    [
      'a system id alone is marked SYSTEM',
      { name: 'html', publicId: '', systemId: 'about:legacy-compat' },
      '<!DOCTYPE html SYSTEM "about:legacy-compat">\n',
    ],
  ]
  for (const [what, doctype, prologue] of doctypes) {
    test(`${what}`, () => {
      const [{ html }] = capture({ doctype }).sent
      assert.equal(html, prologue + CLEAN)
    })
  }
})

describe('the size cap', () => {
  test('a capture of exactly 4,000,000 characters is whole', () => {
    const [body] = capture({ root: rootOf('x'.repeat(MAX)) }).sent
    assert.equal(body.html.length, MAX)
    assert.equal(body.truncated, false)
  })

  test('a longer capture is cut at 4,000,000 characters and marked truncated', () => {
    const [body] = capture({ root: rootOf('x'.repeat(MAX + 1)) }).sent
    assert.equal(body.html.length, MAX)
    assert.equal(body.truncated, true)
  })

  test('the doctype counts toward the cap', () => {
    const doctype = { name: 'html', publicId: '', systemId: '' }
    const [body] = capture({ root: rootOf('x'.repeat(MAX)), doctype }).sent
    assert.equal(body.truncated, true)
    assert.ok(body.html.startsWith('<!DOCTYPE html>\n'))
  })

  test('the cut never ends on half of a surrogate pair', () => {
    const [body] = capture({ root: rootOf('x'.repeat(MAX - 1) + '\u{1F600}y') }).sent
    assert.equal(body.truncated, true)
    assert.equal(body.html, 'x'.repeat(MAX - 1))
  })
})

describe('failures still answer', () => {
  test('a document with no root element answers with empty html', () => {
    assert.deepEqual(capture({ root: null }).sent, [{ html: '', truncated: false }])
  })

  test('a capture that throws answers with empty html rather than leaving the chrome to time out', () => {
    const root = {
      cloneNode: () => {
        throw new Error('detached')
      },
    }
    assert.deepEqual(capture({ root }).sent, [{ html: '', truncated: false }])
  })
})

describe('the channel', () => {
  test('without the webkit handler it posts a JSON string to window.wisPageSource', () => {
    const { sent } = capture({ channel: 'window' })
    assert.equal(typeof sent[0], 'string')
    assert.deepEqual(JSON.parse(sent[0]), { html: CLEAN, truncated: false })
  })

  test('with no channel at all nothing is posted and nothing throws', () => {
    assert.doesNotThrow(() => capture({ channel: 'none' }))
  })

  test('a throwing handler is tolerated', () => {
    const window = { webkit: { messageHandlers: { wisPageSource: { postMessage: () => { throw new Error('gone') } } } } }
    window.top = window.self = window
    const document = { documentElement: pageWithWisNodes(), doctype: null }
    assert.doesNotThrow(() => new Function('window', 'document', SCRIPT)(window, document))
  })

  // macOS reads an NSDictionary; the Windows and Linux dispatchers take the
  // object too (`page_message::json_body`, tested on the Rust side).
  test('with the webkit handler it posts the object itself', () => {
    const { sent } = capture()
    assert.equal(typeof sent[0], 'object')
    assert.deepEqual(sent[0], { html: CLEAN, truncated: false })
  })
})
