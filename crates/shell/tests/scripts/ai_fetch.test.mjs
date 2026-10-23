// ai/fetch.rs: search_script (DuckDuckGo results as JSON) and page_script (main text) for the hidden fetch webview.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'
import { h, handlers, page } from '../support/s2-dom.mjs'
import { rawStringInFn } from '../support/s2-rust.mjs'

const SEARCH = rawStringInFn('ai/fetch.rs', 'search_script')
// page_script(max) is MAIN_CONTENT_FINDER, then the extraction with MAX_CHARS replaced by `max`.
const pageScript = (max) =>
  `${rawStringConst('pages/reader.rs', 'MAIN_CONTENT_FINDER')}\n${rawStringInFn('ai/fetch.rs', 'page_script').replaceAll('MAX_CHARS', String(max))}`
const PAGE = pageScript(8000)

/** Run `script` over a page; returns what it posted to wisAiFetch. */
function run(script, children, { channel = handlers('wisAiFetch'), title = '', setup } = {}) {
  const document = page('https://html.duckduckgo.com/html/?q=x', ...children)
  document.title = title
  const window = channel ? { webkit: channel.webkit } : {}
  setup?.(document)
  new Function('window', 'document', script)(window, document)
  return channel ? channel.sent : []
}

/** One DuckDuckGo result row. */
function row(href, title, snippet, cls = 'result results_links web-result') {
  return h(
    'div',
    { class: cls },
    h('h2', { class: 'result__title' }, h('a', { class: 'result__a', href }, title)),
    snippet == null ? [] : h('a', { class: 'result__snippet', href }, snippet),
  )
}

const redirect = (target) => `//duckduckgo.com/l/?uddg=${encodeURIComponent(target)}&rut=abc`

/** The results the search script posted, parsed as the native side does. */
function results(children, options) {
  const sent = run(SEARCH, children, options)
  assert.equal(sent.length, 1)
  assert.equal(sent[0].body.title, '')
  return JSON.parse(sent[0].body.text)
}

describe('search_script', () => {
  test('each organic result posts its title, decoded target URL and snippet', () => {
    const sent = run(SEARCH, [row(redirect('https://example.com/a?b=1'), ' Example ', ' A snippet ')])
    assert.deepEqual(sent, [
      {
        name: 'wisAiFetch',
        body: {
          text: JSON.stringify([{ title: 'Example', url: 'https://example.com/a?b=1', snippet: 'A snippet' }]),
          title: '',
        },
      },
    ])
  })

  test('the uddg target is read after ? or &', () => {
    const href = `/l/?kh=-1&uddg=${encodeURIComponent('https://example.org/')}`
    assert.equal(results([row(href, 't', 's')])[0].url, 'https://example.org/')
  })

  test('ad rows are skipped', () => {
    const found = results([row(redirect('https://ad.test/'), 'Ad', 's', 'result result--ad'), row(redirect('https://ok.test/'), 'Ok', 's')])
    assert.deepEqual(
      found.map((r) => r.url),
      ['https://ok.test/'],
    )
  })

  test('a row without a result link is skipped', () => {
    const found = results([h('div', { class: 'result' }, h('p', {}, 'no link')), row(redirect('https://ok.test/'), 'Ok', 's')])
    assert.equal(found.length, 1)
  })

  test('at most six results are posted, and skipped ads do not count', () => {
    const rows = [
      row(redirect('https://ad1.test/'), 'Ad', 's', 'result result--ad'),
      row(redirect('https://ad2.test/'), 'Ad', 's', 'result result--ad'),
      ...Array.from({ length: 8 }, (_, i) => row(redirect(`https://r${i}.test/`), `R${i}`, 's')),
    ]
    assert.deepEqual(
      results(rows).map((r) => r.title),
      ['R0', 'R1', 'R2', 'R3', 'R4', 'R5'],
    )
  })

  test('a protocol-relative link becomes https, and any other href is passed on as written', () => {
    const found = results([row('//example.org/x', 'a', 's'), row('https://example.net/', 'b', 's'), row('/relative', 'c', 's')])
    assert.deepEqual(
      found.map((r) => r.url),
      ['https://example.org/x', 'https://example.net/', '/relative'],
    )
  })

  test('an undecodable uddg value keeps the raw href', () => {
    const href = '//duckduckgo.com/l/?uddg=%E0%A4%A'
    assert.equal(results([row(href, 't', 's')])[0].url, href)
  })

  test('a link without an href attribute gives an empty URL', () => {
    const bare = h('div', { class: 'result' }, h('a', { class: 'result__a' }, 'Title'))
    assert.equal(results([bare])[0].url, '')
  })

  test('titles, URLs and snippets are capped at 300, 2000 and 500 characters', () => {
    const [only] = results([row(`https://long.test/${'p'.repeat(3000)}`, 't'.repeat(400), 's'.repeat(600))])
    assert.equal(only.title.length, 300)
    assert.equal(only.url.length, 2000)
    assert.equal(only.snippet.length, 500)
  })

  test('a missing snippet is an empty string', () => {
    assert.equal(results([row(redirect('https://a.test/'), 'a', null)])[0].snippet, '')
  })

  test('a page with no results posts an empty array', () => {
    assert.deepEqual(results([h('p', {}, 'No results.')]), [])
  })

  test('a page that breaks the scrape still posts an empty array', () => {
    const broken = (document) => {
      document.querySelectorAll = () => {
        throw new Error('broken')
      }
    }
    assert.deepEqual(results([], { setup: broken }), [])
  })

  test('without the wisAiFetch channel nothing is posted and nothing throws', () => {
    assert.doesNotThrow(() => run(SEARCH, [row(redirect('https://a.test/'), 'a', 's')], { channel: null }))
    assert.doesNotThrow(() => run(SEARCH, [], { channel: null, setup: (d) => (d.querySelectorAll = null) }))
  })
})

describe('page_script', () => {
  test('posts the main content text, spaces collapsed and line breaks kept, with the trimmed document title', () => {
    const article = h('article', {}, h('p', {}, `Main   story \n\n\t ${'word '.repeat(60)}`))
    const sent = run(PAGE, [h('nav', {}, 'Menu'), article], { title: '  Story title ' })
    assert.equal(sent.length, 1)
    assert.equal(sent[0].body.title, 'Story title')
    assert.equal(sent[0].body.text, `Main story\n${'word '.repeat(60).trim()}`)
  })

  test('the text is capped at the requested length', () => {
    const article = () => h('article', {}, 'x'.repeat(25_000))
    assert.equal(run(PAGE, [article()])[0].body.text.length, 8000)
    assert.equal(run(pageScript(20_000), [article()])[0].body.text.length, 20_000)
  })

  test('an empty page posts empty text', () => {
    const sent = run(PAGE, [])
    assert.deepEqual(sent[0].body, { text: '', title: '' })
  })

  test('without the wisAiFetch channel nothing is posted and nothing throws', () => {
    assert.doesNotThrow(() => run(PAGE, [h('article', {}, 'x')], { channel: null }))
  })

  test('a page that breaks the extraction posts nothing and does not throw', () => {
    const broken = (document) => {
      document.querySelectorAll = () => {
        throw new Error('broken')
      }
    }
    assert.deepEqual(run(PAGE, [], { setup: broken }), [])
  })
})
