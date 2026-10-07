// What the swipe back/forward script in `swipe_nav.rs` does with wheel sequences, the shell's phase and history signals, and its edge indicator.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'
import { createClock } from '../support/s3-clock.mjs'
import { evalIn, rustLiteral } from '../support/s3-native.mjs'

const SCRIPT = rawStringConst('swipe_nav.rs', 'SWIPE_NAV_JS')
const NATIVE = {
  hasPhase: rustLiteral('swipe_nav.rs', '__wisSwipeHasPhase='),
  began: rustLiteral('swipe_nav.rs', "__wisSwipePhase('began')"),
  ended: rustLiteral('swipe_nav.rs', "__wisSwipePhase('ended')"),
  navState: rustLiteral('swipe_nav.rs', '__wisSwipeNavState('),
  accent: rustLiteral('swipe_nav.rs', '__wisSwipeAccent='),
}
const DEFAULT_ACCENT = '#6b77f8'

function styleObject() {
  const custom = {}
  return {
    setProperty: (name, value) => void (custom[name] = value),
    getPropertyValue: (name) => custom[name] ?? '',
  }
}

function element(tag, props = {}) {
  return {
    nodeType: 1,
    tagName: tag.toUpperCase(),
    style: styleObject(),
    dataset: {},
    children: [],
    parentNode: null,
    appendChild(child) {
      child.parentNode = this
      this.children.push(child)
      return child
    },
    ...props,
  }
}

/** The indicator's `<path>`s, keyed by class, from the markup the script writes. */
function parsePaths(html) {
  const paths = {}
  for (const [, text] of html.matchAll(/<path ([^>]*?)\/>/g)) {
    const attrs = Object.fromEntries([...text.matchAll(/([\w-]+)="([^"]*)"/g)].map(([, k, v]) => [k, v]))
    paths[attrs.class] = {
      style: {},
      setAttribute: (name, value) => void (attrs[name] = String(value)),
      getAttribute: (name) => attrs[name] ?? null,
    }
  }
  return paths
}

function createElement(tag) {
  const el = element(tag)
  if (tag !== 'div') return el
  let paths = {}
  Object.defineProperty(el, 'innerHTML', {
    get: () => el.markup,
    set(markup) {
      el.markup = markup
      paths = parsePaths(markup)
    },
  })
  el.querySelector = (selector) => paths[selector.slice(1)] || null
  el.attachShadow = ({ mode }) => (el.shadowRoot = element('#shadow-root', { mode }))
  return el
}

/**
 * A fresh page running the script. `native` gives it the macOS finger-phase
 * signals; `nav` is the shell's [canGoBack, canGoForward] push, or null.
 */
function loadPage({
  native = true,
  nav = [true, true],
  historyLength = 1,
  accent,
  rootAtStart = true,
  lockedPushState = false,
} = {}) {
  const clock = createClock()
  const listeners = {}
  const navigations = []
  const pushes = []
  const history = {
    length: historyLength,
    back: () => navigations.push('back'),
    forward: () => navigations.push('forward'),
  }
  if (lockedPushState) {
    Object.defineProperty(history, 'pushState', {
      get() {
        throw new Error('SecurityError')
      },
    })
  } else {
    history.pushState = function (...args) {
      pushes.push({ self: this, args })
      return 'pushed'
    }
  }

  const html = element('html', { scrollLeft: 0, scrollWidth: 1280, clientWidth: 1280 })
  const body = html.appendChild(element('body'))
  const content = body.appendChild(element('div'))
  const window = {
    history,
    addEventListener: (type, fn, options) => (listeners[type] ||= []).push({ fn, options }),
  }
  const document = {
    documentElement: rootAtStart ? html : null,
    scrollingElement: html,
    addEventListener: (type, fn) => (listeners[`doc:${type}`] ||= []).push({ fn }),
    createElement,
  }
  const getComputedStyle = (el) => ({ overflowX: el.overflowX || 'visible' })
  const fire = (key, event = {}) => (listeners[key] || []).forEach((l) => l.fn(event))
  const install = () =>
    new Function('window', 'document', 'history', 'getComputedStyle', 'setTimeout', 'clearTimeout', SCRIPT)(
      window,
      document,
      history,
      getComputedStyle,
      clock.setTimeout,
      clock.clearTimeout,
    )

  // The order the tab webview builder injects them in.
  if (accent) evalIn(window, NATIVE.accent.replace('{}', JSON.stringify(accent)))
  if (native) evalIn(window, NATIVE.hasPhase)
  install()

  const page = {
    window,
    document,
    history,
    html,
    body,
    content,
    clock,
    navigations,
    pushes,
    listeners,
    install,
    fire,
    began: () => evalIn(window, NATIVE.began),
    ended: () => evalIn(window, NATIVE.ended),
    navState: (back, fwd) =>
      evalIn(window, NATIVE.navState.replace('{back}', back).replace('{fwd}', fwd)),
    setAccent: (color) => evalIn(window, NATIVE.accent.replace('{}', JSON.stringify(color))),

    wheel(deltaX, deltaY = 0, { cancelable = true, target = content, ...rest } = {}) {
      const event = {
        deltaX,
        deltaY,
        cancelable,
        target,
        ...rest,
        prevented: false,
        stopped: false,
        preventDefault() {
          this.prevented = true
        },
        stopPropagation() {
          this.stopped = true
        },
      }
      fire('wheel', event)
      return event
    },
    swipe: (...deltas) => deltas.map((dx) => page.wheel(dx)),

    /** The indicator as drawn, or null before the first gesture. */
    indicator() {
      const host = html.children.find((c) => c.id === '__wisSwipeNav')
      if (!host) return null
      const ind = host.shadowRoot.children.find((c) => c.className === 'ind')
      const semi = ind.querySelector('.semi')
      const arr = ind.querySelector('.arr')
      return {
        host,
        ind,
        semi,
        arr,
        visible: ind.style.opacity === '1',
        dir: ind.dataset.dir,
        tx: ind.style.getPropertyValue('--tx'),
        filled: semi.style.fillOpacity === '1',
        arrow: arr.getAttribute('stroke'),
      }
    },
  }
  if (nav) page.navState(...nav)
  return page
}

/** A horizontal scroller in the page, with a child to aim wheel events at. */
function scroller(p, props) {
  const el = p.body.appendChild(
    element('div', { overflowX: 'auto', scrollWidth: 2000, clientWidth: 500, scrollLeft: 0, ...props }),
  )
  return el.appendChild(element('span'))
}

/** One native-mode gesture over `target`: finger down, the deltas, lift. */
function gesture(p, deltas, target = p.content) {
  p.began()
  const events = deltas.map((dx) => p.wheel(dx, 0, { target }))
  p.ended()
  return events
}

describe('a wheel sequence', () => {
  test('a leftward swipe past the threshold goes back once, at finger lift', () => {
    const p = loadPage()
    p.began()
    p.swipe(-40, -40, -40)
    assert.deepEqual(p.navigations, [])
    p.ended()
    assert.deepEqual(p.navigations, ['back'])
  })

  test('a rightward swipe past the threshold goes forward', () => {
    const p = loadPage()
    gesture(p, [40, 40, 40])
    assert.deepEqual(p.navigations, ['forward'])
  })

  test('the commit arms at exactly 110px and not before', () => {
    const p = loadPage()
    gesture(p, [-109])
    assert.deepEqual(p.navigations, [])
    gesture(p, [-109, -1])
    assert.deepEqual(p.navigations, ['back'])
  })

  test('a sequence that starts vertical stays a scroll after turning horizontal', () => {
    const p = loadPage()
    p.began()
    const first = p.wheel(-2, 10)
    const later = p.swipe(-60, -60, -60)
    p.ended()
    assert.equal(first.prevented, false)
    assert.ok(later.every((e) => !e.prevented))
    assert.deepEqual(p.navigations, [])
    assert.equal(p.indicator(), null)
  })

  test('equal horizontal and vertical movement counts as vertical', () => {
    const p = loadPage()
    p.began()
    const first = p.wheel(-40, 40)
    p.swipe(-120)
    p.ended()
    assert.equal(first.prevented, false)
    assert.deepEqual(p.navigations, [])
  })

  test('movement under 3px leaves the sequence undecided, and 3px decides it', () => {
    const p = loadPage()
    p.began()
    const tiny = p.wheel(0, 2)
    const swipe = p.wheel(-120)
    p.ended()
    assert.equal(tiny.prevented, false)
    assert.equal(swipe.prevented, true)
    assert.deepEqual(p.navigations, ['back'])

    p.began()
    p.wheel(-1, 2)
    p.wheel(-120)
    p.ended()
    assert.deepEqual(p.navigations, ['back'], 'a 3px vertical start made it a scroll')
  })

  test('a non-cancelable first event makes the whole sequence a scroll', () => {
    const p = loadPage()
    p.began()
    p.wheel(-60, 0, { cancelable: false })
    const later = p.swipe(-60, -60)
    p.ended()
    assert.ok(later.every((e) => !e.prevented))
    assert.deepEqual(p.navigations, [])
  })

  test('a non-cancelable event mid-gesture abandons it and hides the indicator', () => {
    const p = loadPage()
    p.began()
    p.swipe(-60)
    assert.equal(p.indicator().visible, true)
    const passive = p.wheel(-60, 0, { cancelable: false })
    assert.equal(passive.prevented, false)
    assert.equal(p.indicator().visible, false)
    const after = p.wheel(-60)
    p.ended()
    assert.equal(after.prevented, false, 'the rest of the sequence is a scroll')
    assert.deepEqual(p.navigations, [])
  })

  test('gesture events are kept from the page, scroll events are not', () => {
    const p = loadPage()
    const swipe = gesture(p, [-40, -40, -40])
    assert.ok(swipe.every((e) => e.prevented && e.stopped))
    p.began()
    const scroll = p.wheel(0, 40)
    p.ended()
    assert.equal(scroll.prevented || scroll.stopped, false)
  })
})

describe('horizontal scroll ownership', () => {
  test('an element that can still scroll left keeps a back swipe as a scroll', () => {
    const p = loadPage()
    const events = gesture(p, [-60, -60], scroller(p, { scrollLeft: 300 }))
    assert.ok(events.every((e) => !e.prevented))
    assert.deepEqual(p.navigations, [])
  })

  test('an element at its left edge lets a back swipe through', () => {
    const p = loadPage()
    gesture(p, [-60, -60], scroller(p, { scrollLeft: 0 }))
    assert.deepEqual(p.navigations, ['back'])
  })

  test('an element that can still scroll right keeps a forward swipe as a scroll', () => {
    const p = loadPage()
    gesture(p, [60, 60], scroller(p, { scrollLeft: 0 }))
    assert.deepEqual(p.navigations, [])
  })

  test('an element within 1px of its right edge lets a forward swipe through', () => {
    const p = loadPage()
    gesture(p, [60, 60], scroller(p, { scrollLeft: 1499 }))
    assert.deepEqual(p.navigations, ['forward'])
  })

  test('only overflow auto or scroll makes an element own the swipe', () => {
    for (const overflowX of ['hidden', 'visible', 'clip']) {
      const p = loadPage()
      gesture(p, [-60, -60], scroller(p, { overflowX, scrollLeft: 300 }))
      assert.deepEqual(p.navigations, ['back'], overflowX)
    }
    const p = loadPage()
    gesture(p, [-60, -60], scroller(p, { overflowX: 'scroll', scrollLeft: 300 }))
    assert.deepEqual(p.navigations, [], 'scroll')
  })

  test('content overflowing by 1px or less does not count as scrollable', () => {
    const p = loadPage()
    gesture(p, [-60, -60], scroller(p, { scrollWidth: 501, scrollLeft: 1 }))
    assert.deepEqual(p.navigations, ['back'])
  })

  test('a scroller further up the tree from the target owns the swipe', () => {
    const p = loadPage()
    const inner = scroller(p, { scrollLeft: 300 })
    const deep = inner.appendChild(element('em')).appendChild(element('b'))
    gesture(p, [-60, -60], deep)
    assert.deepEqual(p.navigations, [])
  })

  test('a page that scrolls horizontally owns the swipe until it reaches that edge', () => {
    const scrolled = (scrollLeft, deltas) => {
      const p = loadPage()
      Object.assign(p.html, { scrollWidth: 2000, clientWidth: 1280, scrollLeft })
      gesture(p, deltas)
      return p.navigations
    }
    assert.deepEqual(scrolled(100, [-60, -60]), [])
    assert.deepEqual(scrolled(100, [60, 60]), [])
    assert.deepEqual(scrolled(0, [-60, -60]), ['back'])
    assert.deepEqual(scrolled(719, [60, 60]), ['forward'])
  })

  test('the document element stands in when the page has no scrolling element', () => {
    const p = loadPage()
    p.document.scrollingElement = null
    Object.assign(p.html, { scrollWidth: 2000, clientWidth: 1280, scrollLeft: 100 })
    gesture(p, [-60, -60])
    assert.deepEqual(p.navigations, [])
  })

  test.todo(
    'a right-to-left scroller that can still scroll left keeps a back swipe as a scroll (bug: canScroll assumes scrollLeft runs from 0 up, but RTL scrollers run from 0 down to negative values, so a back swipe over an RTL carousel navigates instead of scrolling)',
    () => {
      const p = loadPage()
      gesture(p, [-60, -60], scroller(p, { scrollLeft: -300 }))
      assert.deepEqual(p.navigations, [])
    },
  )

  test.todo(
    'a scroller inside a shadow root keeps the swipe as a scroll (bug: canScroll walks up from e.target, which a window listener sees retargeted to the shadow host, so scrollers inside the shadow tree are never consulted)',
    () => {
      const p = loadPage()
      const host = p.body.appendChild(element('x-carousel'))
      const shadow = element('#shadow-root')
      const track = shadow.appendChild(
        element('div', { overflowX: 'auto', scrollWidth: 2000, clientWidth: 500, scrollLeft: 300 }),
      )
      const slide = track.appendChild(element('div'))
      p.began()
      p.wheel(-60, 0, { target: host, composedPath: () => [slide, track, shadow, host, p.body, p.html] })
      p.wheel(-60, 0, { target: host })
      p.ended()
      assert.deepEqual(p.navigations, [])
    },
  )
})

describe('history availability', () => {
  test('the shell report gates each direction on its own', () => {
    const p = loadPage({ nav: [false, true] })
    const back = gesture(p, [-60, -60])
    assert.ok(back.every((e) => !e.prevented))
    assert.equal(p.indicator(), null)
    gesture(p, [60, 60])
    assert.deepEqual(p.navigations, ['forward'])

    const q = loadPage({ nav: [true, false] })
    gesture(q, [60, 60])
    gesture(q, [-60, -60])
    assert.deepEqual(q.navigations, ['back'])
  })

  test('before the shell reports, more than one history entry allows a swipe', () => {
    const lone = loadPage({ nav: null, historyLength: 1 })
    gesture(lone, [-60, -60])
    gesture(lone, [60, 60])
    assert.deepEqual(lone.navigations, [])

    const p = loadPage({ nav: null, historyLength: 2 })
    gesture(p, [-60, -60])
    assert.deepEqual(p.navigations, ['back'])
  })

  test('pushState still pushes, adds a back entry and clears forward', () => {
    const p = loadPage({ nav: [false, true] })
    const state = { page: 2 }
    assert.equal(p.history.pushState(state, '', '/two'), 'pushed')
    assert.equal(p.pushes.length, 1)
    assert.equal(p.pushes[0].self, p.history)
    assert.deepEqual(p.pushes[0].args, [state, '', '/two'])
    gesture(p, [60, 60])
    gesture(p, [-60, -60])
    assert.deepEqual(p.navigations, ['back'])
  })

  test('a popstate drops the reported state and falls back to history.length', () => {
    const p = loadPage({ nav: [false, false], historyLength: 2 })
    gesture(p, [-60, -60])
    assert.deepEqual(p.navigations, [])
    p.fire('popstate')
    gesture(p, [-60, -60])
    assert.deepEqual(p.navigations, ['back'])
  })

  test('a history that refuses the pushState wrapper still gets swipes', () => {
    const p = loadPage({ lockedPushState: true, nav: null, historyLength: 2 })
    gesture(p, [-60, -60])
    assert.deepEqual(p.navigations, ['back'])
  })
})

describe('finger phases from the shell', () => {
  test('momentum after the lift neither navigates again nor is kept from the page', () => {
    const p = loadPage()
    gesture(p, [-60, -60])
    const tx = p.indicator().tx
    const momentum = p.swipe(-80, -80, -80)
    assert.ok(momentum.every((e) => !e.prevented))
    assert.equal(p.indicator().tx, tx)
    assert.deepEqual(p.navigations, ['back'])
  })

  test('wheel events with no finger down, like a mouse wheel, are ignored', () => {
    const p = loadPage()
    const events = p.swipe(-200, -200)
    assert.ok(events.every((e) => !e.prevented))
    assert.equal(p.indicator(), null)
  })

  test('a repeated lift does not navigate twice', () => {
    const p = loadPage()
    gesture(p, [-60, -60])
    p.ended()
    assert.deepEqual(p.navigations, ['back'])
  })

  test('a new finger down abandons a gesture still in flight', () => {
    const p = loadPage()
    p.began()
    p.swipe(-120)
    assert.equal(p.indicator().filled, true)
    p.began()
    assert.equal(p.indicator().visible, false)
    p.ended()
    assert.deepEqual(p.navigations, [])
  })

  test('a gesture with no wheel event and no lift for 1200ms is abandoned', () => {
    const p = loadPage()
    p.began()
    p.swipe(-120)
    p.clock.advance(1000)
    p.wheel(-1)
    p.clock.advance(1199)
    assert.equal(p.indicator().visible, true, 'the quiet-commit fallback is off in native mode')
    assert.deepEqual(p.navigations, [])
    p.clock.advance(1)
    assert.equal(p.indicator().visible, false)
    p.ended()
    assert.deepEqual(p.navigations, [])
  })

  test('a lift ends a scroll sequence, so the next one is judged afresh', () => {
    const p = loadPage()
    p.began()
    p.wheel(0, 30)
    p.ended()
    gesture(p, [-60, -60])
    assert.deepEqual(p.navigations, ['back'])
  })

  test('the first phase signal switches a page without the flag into native mode', () => {
    const p = loadPage({ native: false })
    p.began()
    p.swipe(-120)
    p.clock.advance(90)
    assert.deepEqual(p.navigations, [], 'no quiet commit once phases arrive')
    p.ended()
    assert.deepEqual(p.navigations, ['back'])
  })
})

describe('without phase signals', () => {
  test('a swipe past the threshold commits after 90ms of wheel quiet', () => {
    const p = loadPage({ native: false })
    p.swipe(-60, -60)
    p.clock.advance(89)
    assert.deepEqual(p.navigations, [])
    p.clock.advance(1)
    assert.deepEqual(p.navigations, ['back'])
    p.clock.advance(1000)
    assert.deepEqual(p.navigations, ['back'])
  })

  test('each wheel event restarts the quiet window', () => {
    const p = loadPage({ native: false })
    p.wheel(-60)
    p.clock.advance(80)
    p.wheel(-60)
    p.clock.advance(80)
    assert.deepEqual(p.navigations, [])
    p.clock.advance(10)
    assert.deepEqual(p.navigations, ['back'])
  })

  test('a short swipe ends without navigating after 90ms of quiet', () => {
    const p = loadPage({ native: false })
    p.wheel(-60)
    p.clock.advance(90)
    assert.deepEqual(p.navigations, [])
    assert.equal(p.indicator().visible, false)
  })

  test('a scroll sequence lasts while its events keep coming, then ends after 90ms of quiet', () => {
    const p = loadPage({ native: false })
    p.wheel(0, 30)
    p.clock.advance(80)
    assert.equal(p.wheel(-120).prevented, false)
    p.clock.advance(89)
    assert.equal(p.wheel(-120).prevented, false)
    p.clock.advance(90)
    assert.equal(p.wheel(-120).prevented, true)
    p.clock.advance(90)
    assert.deepEqual(p.navigations, ['back'])
  })

  test.todo(
    'a shift+wheel from a mouse at the page edge stays a scroll (bug: modifier keys are never checked, so in the no-phase fallback used on Windows and Linux a shift+wheel, which the engine delivers as deltaX, navigates back or forward)',
    () => {
      const p = loadPage({ native: false })
      p.wheel(-100, 0, { shiftKey: true })
      p.wheel(-100, 0, { shiftKey: true })
      p.clock.advance(90)
      assert.deepEqual(p.navigations, [])
    },
  )
})

describe('accumulation', () => {
  test('swiping back toward the start reduces progress and unarms the commit', () => {
    const p = loadPage()
    p.began()
    p.swipe(-120)
    assert.equal(p.indicator().filled, true)
    p.wheel(30)
    assert.equal(p.indicator().filled, false)
    assert.equal(p.indicator().arrow, DEFAULT_ACCENT)
    p.ended()
    assert.deepEqual(p.navigations, [])
  })

  test('progress stops at zero, so overshooting the start keeps the direction', () => {
    const p = loadPage()
    p.began()
    p.swipe(-40, 200)
    assert.equal(p.indicator().tx, '-30px')
    assert.equal(p.indicator().dir, 'back')
    p.swipe(-110)
    p.ended()
    assert.deepEqual(p.navigations, ['back'])
  })
})

describe('the indicator', () => {
  test('it is mounted once, in an open shadow root fixed over the page', () => {
    const p = loadPage()
    gesture(p, [-60])
    gesture(p, [60])
    const hosts = p.html.children.filter((c) => c.id === '__wisSwipeNav')
    assert.equal(hosts.length, 1)
    const { host } = p.indicator()
    assert.match(host.style.cssText, /position:fixed/)
    assert.match(host.style.cssText, /pointer-events:none/)
    assert.match(host.style.cssText, /z-index:2147483647/)
    assert.equal(host.shadowRoot.mode, 'open')
    assert.match(host.shadowRoot.children[0].textContent, /\.ind\{/)
  })

  test('a back indicator sits at the left edge and a forward one at the right', () => {
    const p = loadPage()
    p.began()
    p.wheel(-20)
    const back = p.indicator()
    assert.deepEqual([back.dir, back.ind.style.left, back.ind.style.right], ['back', '0', 'auto'])
    p.began()
    p.wheel(20)
    const fwd = p.indicator()
    assert.deepEqual([fwd.dir, fwd.ind.style.left, fwd.ind.style.right], ['forward', 'auto', '0'])
  })

  test('it slides in with progress and sits flush from the threshold on', () => {
    const p = loadPage()
    p.began()
    p.wheel(-55)
    assert.equal(p.indicator().tx, '-15px')
    p.wheel(-55)
    assert.equal(p.indicator().tx, '0px')
    p.wheel(-110)
    assert.equal(p.indicator().tx, '0px')

    p.began()
    p.wheel(55)
    assert.equal(p.indicator().tx, '15px')
  })

  test('arming fills it and gives the arrow a contrasting colour', () => {
    const p = loadPage()
    p.began()
    p.wheel(-60)
    const before = p.indicator()
    assert.equal(before.visible, true)
    assert.equal(before.filled, false)
    assert.equal(before.arrow, DEFAULT_ACCENT)
    assert.equal(before.semi.getAttribute('stroke'), DEFAULT_ACCENT)
    assert.equal(before.semi.getAttribute('fill'), DEFAULT_ACCENT)
    p.wheel(-60)
    assert.equal(p.indicator().filled, true)
    assert.equal(p.indicator().arrow, '#ffffff')
  })

  test('a light accent gets a black arrow, and short hex colours are expanded', () => {
    const armedArrow = (accent) => {
      const p = loadPage({ accent })
      p.began()
      p.wheel(-120)
      return p.indicator().arrow
    }
    assert.equal(armedArrow('#ff0'), '#000000')
    assert.equal(armedArrow('#ffffff'), '#000000')
    assert.equal(armedArrow('#000'), '#ffffff')
  })

  test('an accent that is not a hex colour falls back to a white arrow', () => {
    const p = loadPage({ accent: 'rebeccapurple' })
    p.began()
    p.wheel(-120)
    assert.equal(p.indicator().arrow, '#ffffff')
    assert.equal(p.indicator().semi.getAttribute('fill'), 'rebeccapurple')
  })

  test('the shell accent colours it, and an accent change reaches the next gesture', () => {
    const p = loadPage({ accent: '#ff0000' })
    p.began()
    p.wheel(-20)
    assert.equal(p.indicator().semi.getAttribute('stroke'), '#ff0000')
    p.setAccent('#00ff00')
    p.began()
    p.wheel(-20)
    assert.equal(p.indicator().semi.getAttribute('stroke'), '#00ff00')
    assert.equal(p.indicator().arrow, '#00ff00')
  })

  test('committing snaps the fill without a transition, then holds it for 150ms', () => {
    const p = loadPage()
    gesture(p, [-60, -60])
    const held = p.indicator()
    assert.equal(held.semi.style.transition, 'none')
    assert.equal(held.arr.style.transition, 'none')
    assert.equal(held.filled, true)
    assert.equal(held.visible, true)
    p.clock.advance(149)
    assert.equal(p.indicator().visible, true)
    p.clock.advance(1)
    assert.equal(p.indicator().visible, false)
  })

  test('an unarmed lift hides it at once', () => {
    const p = loadPage()
    gesture(p, [-60])
    assert.equal(p.indicator().visible, false)
  })

  test('a gesture started during the hold is not hidden when the hold runs out', () => {
    const p = loadPage()
    gesture(p, [-120])
    p.clock.advance(50)
    p.began()
    p.wheel(-20)
    p.clock.advance(150)
    const next = p.indicator()
    assert.equal(next.visible, true)
    assert.equal(next.filled, false, 'it starts unfilled')
    assert.equal(next.semi.style.transition, undefined)
  })

  test('pagehide drops the gesture and hides the indicator, mid-gesture or in the hold', () => {
    const p = loadPage()
    p.began()
    p.swipe(-120)
    p.fire('pagehide')
    assert.equal(p.indicator().visible, false)
    p.ended()
    assert.deepEqual(p.navigations, [])

    gesture(p, [-120])
    p.fire('pagehide')
    assert.equal(p.indicator().visible, false)
  })
})

describe('install', () => {
  test('the script installs once per page, with a capturing non-passive wheel listener', () => {
    const p = loadPage()
    const setNav = p.window.__wisSwipeNavState
    p.install()
    assert.equal(p.listeners.wheel.length, 1)
    assert.deepEqual(p.listeners.wheel[0].options, { passive: false, capture: true })
    assert.equal(p.window.__wisSwipeNavState, setNav)
  })

  test('overscroll is locked on the root at once, or at DOMContentLoaded when the root comes later', () => {
    const p = loadPage()
    assert.equal(p.html.style.overscrollBehaviorX, 'none')

    const late = loadPage({ rootAtStart: false })
    assert.equal(late.html.style.overscrollBehaviorX, undefined)
    late.document.documentElement = late.html
    late.fire('doc:DOMContentLoaded')
    assert.equal(late.html.style.overscrollBehaviorX, 'none')
  })
})
