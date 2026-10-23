// pages/find.rs: FIND_ENGINE_JS, the Windows find-in-page engine on the CSS Custom Highlight API.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'
import { Highlight, NodeFilter, h, handlers, page } from '../support/s2-dom.mjs'

const SCRIPT = rawStringConst('pages/find.rs', 'FIND_ENGINE_JS')

/**
 * A page running the engine over `children`. Options drop the Highlight API
 * (`highlight: false`, `css: false`), the result channel, or set the viewport.
 */
function loadPage(children = [], { highlight = true, css = true, handler = true, innerHeight = 800, setup } = {}) {
  const document = page('https://docs.test/', ...children)
  const channel = handlers('wisFindResult')
  const CSS = css ? { highlights: new Map() } : undefined
  const window = { innerHeight, CSS }
  if (handler) window.webkit = channel.webkit
  setup?.(document, window)
  const run = () =>
    new Function('window', 'document', 'CSS', 'Highlight', 'NodeFilter', SCRIPT)(
      window,
      document,
      CSS,
      highlight ? Highlight : undefined,
      NodeFilter,
    )
  run()
  const painted = (name) => CSS?.highlights.get(name)?.ranges ?? null
  return {
    window,
    document,
    run,
    CSS,
    sent: () => channel.sent.map((m) => m.body),
    last: () => channel.sent.at(-1)?.body,
    search: (query, backwards = false, caseSensitive = false, colors) =>
      window.__wisFindSearch(query, backwards, caseSensitive, colors),
    clear: () => window.__wisFindClear(),
    matches: () => painted('wis-find')?.map((r) => r.toString()) ?? null,
    current: () => painted('wis-find-current')?.[0] ?? null,
    all: () => painted('wis-find'),
    style: () => document.getElementById('wis-find-style'),
  }
}

describe('FIND_ENGINE_JS search', () => {
  test('a search reports the match count, selects the first match and paints every match', () => {
    const p = loadPage([h('p', {}, 'Apple pie and apple tart')])
    p.search('apple')
    assert.deepEqual(p.sent(), [{ query: 'apple', found: true, total: 2, index: 1 }])
    assert.deepEqual(p.matches(), ['Apple', 'apple'])
    assert.equal(p.current(), p.all()[0])
  })

  test('matching ignores ASCII case unless asked to match it', () => {
    const p = loadPage([h('p', {}, 'Apple pie and apple tart')])
    p.search('apple', false, true)
    assert.deepEqual(p.last(), { query: 'apple', found: true, total: 1, index: 1 })
    assert.deepEqual(p.matches(), ['apple'])
  })

  test('case folding is ASCII only, so accented capitals do not fold', () => {
    const p = loadPage([h('p', {}, 'École')])
    p.search('école')
    assert.deepEqual(p.last(), { query: 'école', found: false, total: 0, index: -1 })
  })

  test('matches across text nodes are collected in document order and never overlap', () => {
    const first = h('p', {}, 'one cat')
    const second = h('span', {}, 'cat cat')
    const p = loadPage([first, h('div', {}, second), h('p', {}, 'aaaa')])
    p.search('cat')
    assert.equal(p.last().total, 3)
    assert.deepEqual(
      p.all().map((r) => r.startContainer.parentElement),
      [first, second, second],
    )
    p.search('aa')
    assert.equal(p.last().total, 2)
  })

  test('script, style, noscript and textarea text and whitespace-only nodes are not searched', () => {
    const p = loadPage([
      h('script', {}, 'var needle'),
      h('style', {}, '.needle{}'),
      h('noscript', {}, 'needle'),
      h('textarea', {}, 'needle'),
      h('p', {}, '   '),
      h('p', {}, 'a needle'),
    ])
    p.search('needle')
    assert.equal(p.last().total, 1)
  })

  test('text split across elements is not matched', () => {
    const p = loadPage([h('p', {}, h('b', {}, 'fo'), 'o')])
    p.search('foo')
    assert.equal(p.last().found, false)
  })

  test('collection stops at 1000 matches', () => {
    const p = loadPage([h('p', {}, 'a'.repeat(700)), h('p', {}, 'a'.repeat(700))])
    p.search('a')
    assert.deepEqual(p.last(), { query: 'a', found: true, total: 1000, index: 1 })
  })

  test('without a body the whole document is searched', () => {
    const p = loadPage([], {
      setup: (document) => {
        document.documentElement.appendChild(h('div', {}, 'framed needle'))
        document.body.remove()
        document.body = null
      },
    })
    p.search('needle')
    assert.equal(p.last().total, 1)
  })

  test('a match whose range cannot be built is skipped, not fatal', () => {
    const p = loadPage([h('p', {}, 'needle')], {
      setup: (document) => {
        document.createRange = () => {
          throw new Error('detached')
        }
      },
    })
    p.search('needle')
    assert.deepEqual(p.last(), { query: 'needle', found: false, total: 0, index: -1 })
  })
})

describe('FIND_ENGINE_JS stepping', () => {
  test('repeating the query steps to the next match and wraps to the first', () => {
    const p = loadPage([h('p', {}, 'x x x')])
    p.search('x')
    p.search('x')
    p.search('x')
    p.search('x')
    assert.deepEqual(
      p.sent().map((m) => m.index),
      [1, 2, 3, 1],
    )
    assert.equal(p.current(), p.all()[0])
  })

  test('stepping backwards goes to the previous match and wraps to the last', () => {
    const p = loadPage([h('p', {}, 'x x x')])
    p.search('x')
    p.search('x', true)
    p.search('x', true)
    assert.deepEqual(
      p.sent().map((m) => m.index),
      [1, 3, 2],
    )
    assert.equal(p.current(), p.all()[1])
  })

  test('a fresh backwards search starts at the last match', () => {
    const p = loadPage([h('p', {}, 'x x x')])
    p.search('x', true)
    assert.deepEqual(p.last(), { query: 'x', found: true, total: 3, index: 3 })
  })

  test('changing case sensitivity starts a fresh search', () => {
    const p = loadPage([h('p', {}, 'Apple apple')])
    p.search('Apple')
    p.search('Apple')
    assert.equal(p.last().index, 2)
    p.search('Apple', false, true)
    assert.deepEqual(p.last(), { query: 'Apple', found: true, total: 1, index: 1 })
  })

  test('repeating a query that found nothing searches the page again', () => {
    const p = loadPage([h('p', {}, 'nothing yet')])
    p.search('later')
    p.document.body.appendChild(h('p', {}, 'later text'))
    p.search('later')
    assert.deepEqual(p.last(), { query: 'later', found: true, total: 1, index: 1 })
  })

  test('a query with no matches clears the previous highlights and reports not found', () => {
    const p = loadPage([h('p', {}, 'apple')])
    p.search('apple')
    p.search('zzz')
    assert.deepEqual(p.last(), { query: 'zzz', found: false, total: 0, index: -1 })
    assert.equal(p.CSS.highlights.size, 0)
  })

  test('an empty or missing query reports not found with an empty query', () => {
    const p = loadPage([h('p', {}, 'apple')])
    p.search('')
    p.search(null)
    assert.deepEqual(p.sent(), [
      { query: '', found: false, total: 0, index: -1 },
      { query: '', found: false, total: 0, index: -1 },
    ])
  })
})

describe('FIND_ENGINE_JS scrolling', () => {
  function scrolled(rect, options) {
    const holder = h('p', {}, 'needle')
    holder.rect = rect
    const p = loadPage([holder], options)
    p.search('needle')
    return holder.scrolls
  }

  test('the current match is centred only when it is outside the viewport', () => {
    assert.deepEqual(scrolled({ top: 100, bottom: 120 }), [])
    assert.deepEqual(scrolled({ top: 2000, bottom: 2020 }), [{ block: 'center', inline: 'nearest' }])
    assert.deepEqual(scrolled({ top: -40, bottom: -20 }), [{ block: 'center', inline: 'nearest' }])
  })

  test('without a window height the root element height is the viewport', () => {
    const tall = { innerHeight: 0, setup: (document) => (document.documentElement.clientHeight = 500) }
    assert.deepEqual(scrolled({ top: 480, bottom: 520 }, tall), [{ block: 'center', inline: 'nearest' }])
    assert.deepEqual(scrolled({ top: 400, bottom: 420 }, tall), [])
  })

  test('stepping scrolls to the new current match', () => {
    const near = h('p', {}, 'needle')
    const far = h('p', {}, 'needle')
    near.rect = { top: 10, bottom: 30 }
    far.rect = { top: 5000, bottom: 5020 }
    const p = loadPage([near, far])
    p.search('needle')
    assert.equal(far.scrolls.length, 0)
    p.search('needle')
    assert.equal(far.scrolls.length, 1)
  })
})

describe('FIND_ENGINE_JS painting', () => {
  test('the highlight style uses the colours passed in', () => {
    const p = loadPage([h('p', {}, 'x')])
    p.search('x', false, false, { match: 'rgba(1, 2, 3, 0.35)', current: 'rgb(1, 2, 3)', text: '#000' })
    assert.equal(
      p.style().textContent,
      '::highlight(wis-find){background:rgba(1, 2, 3, 0.35)}' +
        '::highlight(wis-find-current){background:rgb(1, 2, 3);color:#000}',
    )
    assert.equal(p.style().parentNode, p.document.head)
  })

  test('without colours the Windows default accent is used', () => {
    const p = loadPage([h('p', {}, 'x')])
    p.search('x')
    assert.equal(
      p.style().textContent,
      '::highlight(wis-find){background:rgba(0, 120, 212, 0.35)}' +
        '::highlight(wis-find-current){background:rgb(0, 120, 212);color:#fff}',
    )
  })

  test('the style element is made once and rewritten only when the colours change', () => {
    const p = loadPage([h('p', {}, 'x')])
    p.search('x')
    const style = p.style()
    let writes = 0
    let text = style.textContent
    Object.defineProperty(style, 'textContent', {
      get: () => text,
      set: (value) => {
        writes++
        text = value
      },
    })
    p.search('x')
    assert.equal(writes, 0)
    p.search('x', false, false, { match: 'red', current: 'blue', text: '#000' })
    assert.equal(writes, 1)
    assert.equal(p.document.querySelectorAll('style').length, 1)
  })

  test('without a head the style goes under the root element', () => {
    const p = loadPage([h('p', {}, 'x')], { setup: (document) => (document.head = null) })
    p.search('x')
    assert.equal(p.style().parentNode, p.document.documentElement)
  })

  test('without the Highlight API the search still counts but paints nothing', () => {
    for (const missing of [{ highlight: false }, { css: false }]) {
      const p = loadPage([h('p', {}, 'x x')], missing)
      assert.doesNotThrow(() => p.search('x'))
      assert.deepEqual(p.last(), { query: 'x', found: true, total: 2, index: 1 })
      assert.equal(p.CSS?.highlights.size ?? 0, 0)
      assert.doesNotThrow(() => p.clear())
    }
  })

  test('a highlight registry that throws does not stop the result being posted', () => {
    const p = loadPage([h('p', {}, 'x')])
    p.CSS.highlights.set = () => {
      throw new Error('registry')
    }
    p.search('x')
    assert.deepEqual(p.last(), { query: 'x', found: true, total: 1, index: 1 })
  })
})

describe('FIND_ENGINE_JS lifecycle', () => {
  test('clear removes the highlights and forgets the query, so the next search starts over', () => {
    const p = loadPage([h('p', {}, 'x x x')])
    p.search('x')
    p.search('x')
    p.clear()
    assert.equal(p.CSS.highlights.size, 0)
    p.search('x')
    assert.equal(p.last().index, 1)
  })

  test('without the result channel the search still paints', () => {
    const p = loadPage([h('p', {}, 'x')], { handler: false })
    assert.doesNotThrow(() => p.search('x'))
    assert.deepEqual(p.matches(), ['x'])
  })

  test('installing twice keeps the first engine and its state', () => {
    const p = loadPage([h('p', {}, 'x x')])
    const search = p.window.__wisFindSearch
    p.search('x')
    p.run()
    assert.equal(p.window.__wisFindSearch, search)
    p.search('x')
    assert.equal(p.last().index, 2)
  })
})
