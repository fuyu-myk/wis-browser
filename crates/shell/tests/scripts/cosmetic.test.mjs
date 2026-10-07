// cosmetic.rs: COSMETIC_JS (class/id collector and CSS applier) and PICKER_JS (element zapper).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'
import { eventTarget, h, handlers, makeLocation, page } from '../support/s2-dom.mjs'
import { fnBody } from '../support/s2-rust.mjs'

const COSMETIC = rawStringConst('cosmetic.rs', 'COSMETIC_JS')
const PICKER = rawStringConst('cosmetic.rs', 'PICKER_JS')

// Windows and Linux inject COSMETIC_JS through `page_script()`, which wraps it
// in a top-frame check built from two string literals.
const [WRAP_OPEN, WRAP_CLOSE] = [...fnBody('cosmetic.rs', 'page_script').matchAll(/"((?:[^"\\]|\\.)*)"/g)].map(
  (m) => JSON.parse(`"${m[1]}"`),
)
const WRAPPED = `${WRAP_OPEN}${COSMETIC}${WRAP_CLOSE}`

const URL_NOW = 'https://news.test/'

/** A page running the collector. `setup(document, window)` runs before the script. */
function loadCosmetic({ children = [], handler = true, observer = true, top = true, script = COSMETIC, setup } = {}) {
  const document = page(URL_NOW, ...children)
  const location = document.location
  const channel = handlers('wisCosmetic')
  const window = eventTarget({})
  window.self = window
  window.top = top ? window : {}
  if (handler) window.webkit = channel.webkit
  let timers = []
  const setTimeout = (fn, ms) => timers.push({ fn, ms })
  const observers = []
  class MutationObserver {
    constructor(callback) {
      this.callback = callback
      observers.push(this)
    }
    observe(target, options) {
      this.target = target
      this.options = options
    }
  }
  const navigate = (url) => Object.assign(location, makeLocation(new URL(url, location.href).href))
  const history = {
    pushState: (_state, _title, url) => (navigate(url), 'pushed'),
    replaceState: (_state, _title, url) => (navigate(url), 'replaced'),
  }
  setup?.(document, window)
  const run = () =>
    new Function('window', 'document', 'location', 'history', 'setTimeout', 'MutationObserver', script)(
      window,
      document,
      location,
      history,
      setTimeout,
      observer ? MutationObserver : undefined,
    )
  run()
  return {
    window,
    document,
    history,
    observers,
    run,
    navigate,
    sent: () => channel.sent.map((m) => m.body),
    attachHandler: () => (window.webkit = channel.webkit),
    add: (...els) => els.forEach((el) => document.body.appendChild(el)),
    mutate: () => observers.forEach((o) => o.callback([])),
    delays: () => timers.map((t) => t.ms),
    flush: () => {
      const due = timers
      timers = []
      due.forEach((t) => t.fn())
    },
    apply: (payload) => window.__wisCosmeticApply(payload),
    style: (id) => document.getElementById(id),
  }
}

describe('COSMETIC_JS collection', () => {
  test('the first scan posts the page URL with every class and id once, marked initial', () => {
    const p = loadCosmetic({
      children: [h('div', { id: 'hero', class: 'ad banner' }), h('span', { class: 'ad' }), h('p', { id: 'hero' })],
    })
    assert.deepEqual(p.sent(), [{ href: URL_NOW, initial: true, classes: ['ad', 'banner'], ids: ['hero'] }])
  })

  test('a page with no classes or ids still posts an initial scan, so host rules apply', () => {
    const p = loadCosmetic()
    assert.deepEqual(p.sent(), [{ href: URL_NOW, initial: true, classes: [], ids: [] }])
  })

  test('page changes are batched into one scan 700 ms later carrying only names not sent before', () => {
    const p = loadCosmetic({ children: [h('div', { class: 'old' })] })
    p.add(h('div', { class: 'old fresh' }), h('p', { id: 'late' }))
    p.mutate()
    p.mutate()
    p.mutate()
    assert.deepEqual(p.delays(), [700])
    p.flush()
    assert.deepEqual(p.sent().at(-1), { href: URL_NOW, initial: false, classes: ['fresh'], ids: ['late'] })
    p.mutate()
    assert.deepEqual(p.delays(), [700], 'the next change schedules a new scan')
  })

  test('a later scan that finds nothing new posts nothing', () => {
    const p = loadCosmetic({ children: [h('div', { class: 'old' })] })
    p.mutate()
    p.flush()
    assert.equal(p.sent().length, 1)
  })

  test('the observer watches the whole document for child, class and id changes', () => {
    const p = loadCosmetic()
    assert.equal(p.observers.length, 1)
    assert.equal(p.observers[0].target, p.document.documentElement)
    assert.deepEqual(p.observers[0].options, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['class', 'id'],
    })
  })

  test('a scan stops at 1500 classes and 1500 ids, and the rest come in the next scan', () => {
    const many = Array.from({ length: 1600 }, (_, i) => h('i', { id: `i${i}`, class: `c${i}` }))
    const p = loadCosmetic({ children: many })
    const [first] = p.sent()
    assert.equal(first.classes.length, 1500)
    assert.equal(first.ids.length, 1500)
    p.mutate()
    p.flush()
    const second = p.sent()[1]
    assert.equal(second.initial, false)
    assert.deepEqual(second.classes, Array.from({ length: 100 }, (_, i) => `c${1500 + i}`))
    assert.deepEqual(second.ids, Array.from({ length: 100 }, (_, i) => `i${1500 + i}`))
  })

  test('a full class list does not stop ids from being collected', () => {
    const crowded = h('div', { class: Array.from({ length: 1600 }, (_, i) => `k${i}`).join(' ') })
    const p = loadCosmetic({ children: [crowded, h('a', { id: 'x' }), h('a', { id: 'y' })] })
    assert.equal(p.sent()[0].classes.length, 1500)
    assert.deepEqual(p.sent()[0].ids, ['x', 'y'])
    p.mutate()
    p.flush()
    assert.equal(p.sent()[1].classes.length, 100)
  })

  test('a page whose query throws still posts an empty initial scan', () => {
    const p = loadCosmetic({
      setup: (document) => {
        document.querySelectorAll = () => {
          throw new Error('broken DOM')
        }
      },
    })
    assert.deepEqual(p.sent(), [{ href: URL_NOW, initial: true, classes: [], ids: [] }])
  })

  test('without the wisCosmetic handler nothing is posted, and scans stay initial until one gets through', () => {
    const p = loadCosmetic({ handler: false, children: [h('div', { class: 'a' })] })
    assert.deepEqual(p.sent(), [])
    p.attachHandler()
    p.add(h('div', { class: 'b' }))
    p.mutate()
    p.flush()
    assert.equal(p.sent().length, 1)
    assert.equal(p.sent()[0].initial, true)
  })

  test('without MutationObserver the first scan still posts', () => {
    const p = loadCosmetic({ observer: false, children: [h('div', { class: 'a' })] })
    assert.deepEqual(p.sent(), [{ href: URL_NOW, initial: true, classes: ['a'], ids: [] }])
  })

  test('installing twice keeps the first install and posts once', () => {
    const p = loadCosmetic()
    const apply = p.window.__wisCosmeticApply
    p.run()
    assert.equal(p.sent().length, 1)
    assert.equal(p.window.__wisCosmeticApply, apply)
    assert.equal(p.observers.length, 1)
  })

  test('on Windows and Linux the collector runs in the top frame only', () => {
    const frame = loadCosmetic({ script: WRAPPED, top: false })
    assert.deepEqual(frame.sent(), [])
    assert.equal(frame.window.__wisCosmeticInstalled, undefined)
    const top = loadCosmetic({ script: WRAPPED })
    assert.equal(top.sent().length, 1)
  })
})

describe('COSMETIC_JS in-page navigation', () => {
  test('a pushState to a new URL posts an initial, empty scan for that URL 350 ms later', () => {
    const p = loadCosmetic({ children: [h('div', { class: 'a' })] })
    p.history.pushState({}, '', '/story/2')
    assert.deepEqual(p.delays(), [350])
    p.flush()
    assert.deepEqual(p.sent().at(-1), { href: 'https://news.test/story/2', initial: true, classes: [], ids: [] })
  })

  test('replaceState and popstate count as navigation too', () => {
    const p = loadCosmetic()
    p.history.replaceState(null, '', '/r')
    p.flush()
    p.navigate('/back')
    p.window.fire('popstate')
    p.flush()
    assert.deepEqual(
      p.sent().slice(1).map((m) => m.href),
      ['https://news.test/r', 'https://news.test/back'],
    )
  })

  test('several navigations in quick succession post once, with the latest URL', () => {
    const p = loadCosmetic()
    p.history.pushState({}, '', '/a')
    p.history.pushState({}, '', '/b')
    assert.deepEqual(p.delays(), [350])
    p.flush()
    assert.deepEqual(
      p.sent().slice(1).map((m) => m.href),
      ['https://news.test/b'],
    )
  })

  test('a history change that keeps the URL is not a navigation', () => {
    const p = loadCosmetic()
    p.history.replaceState({ scroll: 1 }, '', URL_NOW)
    p.window.fire('popstate')
    assert.deepEqual(p.delays(), [])
  })

  test('the wrapped history methods still return what the originals return', () => {
    const p = loadCosmetic()
    assert.equal(p.history.pushState({}, '', '/x'), 'pushed')
    assert.equal(p.history.replaceState({}, '', '/y'), 'replaced')
  })

  test('a navigation without the handler fails quietly', () => {
    const p = loadCosmetic({ handler: false })
    p.history.pushState({}, '', '/x')
    assert.doesNotThrow(() => p.flush())
  })
})

describe('COSMETIC_JS applying CSS', () => {
  test('host CSS goes into its own style element in the head and replaces the previous host CSS', () => {
    const p = loadCosmetic()
    p.apply({ host: '.a{display:none!important;}\n', generic: null })
    const style = p.style('wis-cosmetic-host')
    assert.equal(style.parentNode, p.document.head)
    p.apply({ host: '.b{display:none!important;}\n', generic: null })
    assert.equal(p.style('wis-cosmetic-host'), style)
    assert.equal(style.textContent, '.b{display:none!important;}\n')
  })

  test('an empty host string clears the host rules, a null host leaves them', () => {
    const p = loadCosmetic()
    p.apply({ host: '.a{}', generic: null })
    p.apply({ host: null, generic: '.g{}' })
    assert.equal(p.style('wis-cosmetic-host').textContent, '.a{}')
    p.apply({ host: '', generic: null })
    assert.equal(p.style('wis-cosmetic-host').textContent, '')
  })

  test('generic CSS accumulates in a separate style element, one line per apply', () => {
    const p = loadCosmetic()
    p.apply({ host: null, generic: '.x{}' })
    p.apply({ host: null, generic: '.y{}' })
    assert.equal(p.style('wis-cosmetic').textContent, '.x{}\n.y{}\n')
    assert.equal(p.style('wis-cosmetic-host'), null)
  })

  test('a style element the page removed is recreated on the next apply', () => {
    const p = loadCosmetic()
    p.apply({ host: '.a{}', generic: '.g{}' })
    p.style('wis-cosmetic-host').remove()
    p.style('wis-cosmetic').remove()
    p.apply({ host: '.b{}', generic: '.h{}' })
    assert.equal(p.style('wis-cosmetic-host').textContent, '.b{}')
    assert.equal(p.style('wis-cosmetic').textContent, '.h{}\n')
  })

  test('without a head the styles go under the root element', () => {
    const p = loadCosmetic({ setup: (document) => (document.head = null) })
    p.apply({ host: '.a{}', generic: '.g{}' })
    assert.equal(p.style('wis-cosmetic-host').parentNode, p.document.documentElement)
    assert.equal(p.style('wis-cosmetic').parentNode, p.document.documentElement)
  })

  test('an empty payload does nothing and DOM errors are swallowed', () => {
    const p = loadCosmetic()
    assert.doesNotThrow(() => p.apply(null))
    assert.doesNotThrow(() => p.apply(undefined))
    p.document.createElement = () => {
      throw new Error('no DOM')
    }
    assert.doesNotThrow(() => p.apply({ host: '.a{}', generic: '.g{}' }))
  })
})

/** A page with the picker started. `setup(document, window)` runs before the script. */
function loadPicker({ children = [], handler = true, setup } = {}) {
  const document = page('https://shop.test/deals', ...children)
  const channel = handlers('wisZap')
  const window = {}
  if (handler) window.webkit = channel.webkit
  setup?.(document, window)
  const run = () => new Function('window', 'document', 'location', PICKER)(window, document, document.location)
  run()
  const hover = (el) => {
    document.pointTarget = el
    return document.fire('mousemove', { clientX: 5, clientY: 5 })
  }
  return {
    window,
    document,
    run,
    hover,
    sent: () => channel.sent.map((m) => m.body),
    box: () => document.documentElement.children.find((e) => e.tagName === 'DIV') ?? null,
    click: () => document.fire('click'),
    key: (key) => document.fire('keydown', { key }),
    contextmenu: () => document.fire('contextmenu'),
    pick: (el) => {
      hover(el)
      return document.fire('click')
    },
    listening: () => ['mousemove', 'click', 'keydown', 'contextmenu'].map((t) => document.listenerCount(t)),
    selectorFor: (el) => {
      hover(el)
      document.fire('click')
      return channel.sent.at(-1)?.body.selector ?? null
    },
  }
}

function assertTornDown(p) {
  assert.equal(p.window.__wisPickerActive, false)
  assert.equal(p.window.__wisPickerCancel, null)
  assert.equal(p.box(), null, 'the highlight box is removed')
  assert.deepEqual(p.listening(), [0, 0, 0, 0])
}

describe('PICKER_JS', () => {
  test('starting the picker adds a highlight box and listens in the capture phase', () => {
    const p = loadPicker()
    assert.equal(p.window.__wisPickerActive, true)
    assert.match(p.box().style.cssText, /position:fixed;z-index:2147483647;pointer-events:none/)
    assert.deepEqual(p.listening(), [1, 1, 1, 1])
  })

  test('the box frames the element under the pointer, ignoring itself and empty space', () => {
    const target = h('div', { class: 'ad' })
    target.rect = { left: 10, top: 20, width: 300, height: 250 }
    const p = loadPicker({ children: [target] })
    p.hover(target)
    const frame = () => ['left', 'top', 'width', 'height'].map((k) => p.box().style[k])
    assert.deepEqual(frame(), ['10px', '20px', '300px', '250px'])
    p.hover(p.box())
    p.hover(null)
    assert.deepEqual(frame(), ['10px', '20px', '300px', '250px'])
    p.click()
    assert.equal(p.sent()[0].selector, 'div.ad', 'the last real element stays picked')
  })

  test('clicking hides the element, posts the page URL and its selector, and closes the picker', () => {
    const promo = h('div', { id: 'promo' }, 'Sale')
    const p = loadPicker({ children: [h('main', {}, promo)] })
    const click = p.pick(promo)
    assert.deepEqual(p.sent(), [{ href: 'https://shop.test/deals', selector: '#promo' }])
    assert.equal(promo.style.display, 'none')
    assert.equal(promo.style.priorities.display, 'important')
    assert.ok(click.defaultPrevented && click.propagationStopped, 'the page never sees the click')
    assertTornDown(p)
  })

  test('an id that is not a plain identifier is skipped for the tag and classes', () => {
    const slot = h('div', { id: '1-ad', class: 'slot' })
    const p = loadPicker({ children: [slot] })
    assert.equal(p.selectorFor(slot), 'div.slot')
  })

  test('same-tag siblings are told apart with :nth-of-type', () => {
    const items = [h('li'), h('li'), h('li')]
    const p = loadPicker({ children: [h('ul', {}, items)] })
    assert.equal(p.selectorFor(items[1]), 'li:nth-of-type(2)')
  })

  test('a class plus position is used when the class alone is shared', () => {
    const cards = [h('div', { class: 'card' }), h('div', { class: 'card sponsored' })]
    const p = loadPicker({ children: [h('section', {}, cards)] })
    assert.equal(p.selectorFor(cards[1]), 'div.card.sponsored:nth-of-type(2)')
  })

  test('the path climbs until it is unique, stopping at an id ancestor', () => {
    const mine = h('span')
    const p = loadPicker({
      children: [h('main', { id: 'a' }, h('div', { class: 'box' }, mine)), h('aside', { id: 'b' }, h('div', { class: 'box' }, h('span')))],
    })
    assert.equal(p.selectorFor(mine), '#a > div.box > span')
  })

  test('long, generated-looking classes are dropped and at most two are kept', () => {
    const el = h('div', { class: `${'x'.repeat(40)} 9col hero main extra` })
    const p = loadPicker({ children: [el] })
    assert.equal(p.selectorFor(el), 'div.hero.main')
  })

  test('an SVG element class is read from its baseVal', () => {
    const icon = h('svg', { class: 'icon' })
    Object.defineProperty(icon, 'className', { value: { baseVal: 'icon' } })
    const p = loadPicker({ children: [icon] })
    assert.equal(p.selectorFor(icon), 'svg.icon')
  })

  test('the body, the root, or no hovered element yield no selector but still close the picker', () => {
    for (const pickOf of [(d) => d.body, (d) => d.documentElement, () => undefined]) {
      const p = loadPicker()
      const target = pickOf(p.document)
      if (target) p.hover(target)
      p.click()
      assert.deepEqual(p.sent(), [])
      assertTornDown(p)
    }
  })

  test('a path that is still ambiguous after eight levels gives up', () => {
    const chain = () => {
      const leaf = h('span')
      let top = leaf
      for (let i = 0; i < 9; i++) top = h('div', {}, top)
      return { top, leaf }
    }
    const a = chain()
    const b = chain()
    const p = loadPicker({ children: [a.top, b.top] })
    p.pick(a.leaf)
    assert.deepEqual(p.sent(), [])
    assert.equal(a.leaf.style.display, undefined, 'nothing is hidden')
  })

  test('a selector query that throws counts as not unique', () => {
    const el = h('div')
    const p = loadPicker({
      children: [h('main', {}, el)],
      setup: (document) => {
        document.querySelectorAll = () => {
          throw new SyntaxError('bad selector')
        }
      },
    })
    p.pick(el)
    assert.deepEqual(p.sent(), [])
    assertTornDown(p)
  })

  test.todo(
    'an element whose id is repeated on the page can still be zapped by a path selector (bug: selectorFor stops at the first id it meets, so a duplicate id yields no selector and the click hides nothing)',
  )

  test('Escape cancels without posting; other keys do nothing', () => {
    const p = loadPicker({ children: [h('div', { id: 'x' })] })
    const other = p.key('a')
    assert.equal(other.defaultPrevented, false)
    assert.equal(p.window.__wisPickerActive, true)
    const esc = p.key('Escape')
    assert.ok(esc.defaultPrevented)
    assert.deepEqual(p.sent(), [])
    assertTornDown(p)
  })

  test('a right-click cancels and suppresses the context menu', () => {
    const p = loadPicker()
    const menu = p.contextmenu()
    assert.ok(menu.defaultPrevented)
    assertTornDown(p)
  })

  test('starting the picker while it is open cancels it instead', () => {
    const p = loadPicker()
    p.run()
    assertTornDown(p)
    p.run()
    assert.equal(p.window.__wisPickerActive, true)
    assert.deepEqual(p.listening(), [1, 1, 1, 1])
  })

  test('without the wisZap handler the element is still hidden and the picker closes', () => {
    const el = h('div', { id: 'promo' })
    const p = loadPicker({ handler: false, children: [el] })
    assert.doesNotThrow(() => p.pick(el))
    assert.equal(el.style.display, 'none')
    assertTornDown(p)
  })
})
