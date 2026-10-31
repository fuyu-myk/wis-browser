// The probe in `translation/page/`: after every page load it reports the
// page's declared language and a sample of its text for the address bar's
// offer, and asks for another probe once a single-page app moves to another
// address.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { eventTarget, h, handlers, page } from '../support/s2-dom.mjs'
import { concatenated } from '../support/rustSource.mjs'

const SCRIPT = concatenated('translation/page.rs', 'PROBE_JS')
const TRANSLATOR = concatenated('translation/page.rs', 'PAGE_JS')

/**
 * Runs the probe script over `doc` and returns its probe, what it posted and
 * the page's navigations. `navigation` is the Navigation API, which an older
 * WebKit lacks; `named` stands for the page's elements that window's named
 * access reaches.
 */
function load(doc, { navigation = eventTarget({}), named = {} } = {}) {
  const { webkit, sent } = handlers('wisTranslate')
  const window = eventTarget(Object.assign(Object.create(named), { webkit }))
  const location = { href: doc.location.href }
  const timers = new Map()
  let next = 1
  const setTimeout = (fn) => {
    timers.set(next, fn)
    return next++
  }
  const clearTimeout = (id) => timers.delete(id)
  const run = () =>
    new Function('window', 'document', 'navigation', 'location', 'setTimeout', 'clearTimeout', SCRIPT)(
      window,
      doc,
      navigation,
      location,
      setTimeout,
      clearTimeout,
    )
  run()
  const flush = () => {
    const due = [...timers.values()]
    timers.clear()
    for (const fn of due) fn()
  }
  /** The page moves to `href` without loading a new one, as `pushState` does. */
  const move = (href) => {
    location.href = href
    navigation.fire('navigatesuccess')
  }
  const messages = () => sent.map(({ body }) => JSON.parse(body))
  const bodies = () => sent.map(({ body }) => body)
  return { probe: (id) => window.__wisProbe(id), run, flush, move, messages, bodies, navigation }
}

const moves = (loaded) => loaded.messages().filter((m) => m.kind === 'moved')

describe('the probe sample', () => {
  test('a probe samples headings and paragraphs, not scripts, code or other languages', () => {
    const doc = page(
      'https://example.jp/',
      h('nav', {}, h('a', { href: '/' }, 'ホーム')),
      h('h1', {}, '二次方程式について'),
      h('p', {}, '二次方程式とは、数学において二次の多項式で表される方程式である。'),
      h('pre', {}, h('code', {}, 'let x = 1 // a code sample in English')),
      h('p', { lang: 'en' }, 'An English quotation that should not count.'),
      h('p', {}, h('span', { class: 'notranslate' }, 'Brand name kept as it is in English')),
      h('li', {}, '短'),
    )
    doc.title = '二次方程式 - Wikipedia'
    const loaded = load(doc)
    loaded.probe(5)
    const [message] = loaded.messages()
    assert.deepEqual(Object.keys(message), ['kind', 'id', 'lang', 'sample'])
    assert.deepEqual([message.kind, message.id], ['probe', 5])
    assert.equal(
      message.sample,
      '二次方程式 - Wikipedia\n二次方程式について\n二次方程式とは、数学において二次の多項式で表される方程式である。',
    )
  })

  test('a probe samples the main content first and leaves out text marked as another language', () => {
    const languages = h('ul', {}, h('li', {}, h('a', { href: '/af', lang: 'af' }, 'Afrikaans, die taal')))
    const doc = page(
      'https://example.jp/',
      h('nav', {}, h('p', {}, 'メニューの項目の一覧です')),
      languages,
      h('main', {}, h('p', {}, '本文は', h('span', { lang: 'en' }, 'web browser'), 'についての記事である。')),
    )
    const loaded = load(doc)
    loaded.probe(5)
    assert.equal(loaded.messages()[0].sample, '本文はについての記事である。')
  })

  test('a probe falls back to marked text when that is all the page has', () => {
    const doc = page('https://example.jp/', h('main', {}, h('div', { lang: 'ja' }, h('p', {}, '日本は東アジアの国である。'))))
    const loaded = load(doc)
    loaded.probe(5)
    assert.equal(loaded.messages()[0].sample, '日本は東アジアの国である。')
  })

  test('a probe samples content marked as the language the page declares', () => {
    const doc = page('https://example.jp/', h('main', {}, h('div', { lang: 'ja' }, h('p', {}, '日本は東アジアの国である。'))))
    doc.documentElement.setAttribute('lang', 'ja')
    const loaded = load(doc)
    loaded.probe(5)
    assert.equal(loaded.messages()[0].sample, '日本は東アジアの国である。')
  })

  test('a probe sample leaves out menus, asides, headers and footers', () => {
    const doc = page(
      'https://example.jp/',
      h(
        'main',
        {},
        h('nav', {}, h('li', {}, 'この版への固定リンク')),
        h('aside', {}, h('p', {}, '関連する記事の一覧')),
        h('header', {}, h('p', {}, 'ページの見出し部分')),
        h('p', {}, '本文の最初の段落である。'),
        h('footer', {}, h('p', {}, '最終更新日時の表示')),
      ),
    )
    const loaded = load(doc)
    loaded.probe(5)
    assert.equal(loaded.messages()[0].sample, '本文の最初の段落である。')
  })

  test('a sample cut inside an emoji is still well formed', () => {
    const loaded = load(page('https://example.jp/', h('p', {}, 'あ'.repeat(999) + '😀です')))
    loaded.probe(5)
    assert.ok(!/\\u[dD][89abAB]/.test(loaded.bodies()[0]), 'no lone surrogate')
  })

  test('a probe sample stops at a thousand characters', () => {
    const paragraphs = Array.from({ length: 40 }, (_, i) => h('p', {}, `第${i}段落。`.repeat(10)))
    const loaded = load(page('https://example.jp/', ...paragraphs))
    loaded.probe(5)
    assert.ok(loaded.messages()[0].sample.length <= 1000)
  })
})

describe('moving without loading', () => {
  test('a move to another address asks for another probe once the page settles', () => {
    const loaded = load(page('https://example.jp/'))
    loaded.probe(1)
    loaded.move('https://example.jp/articles/2')
    assert.deepEqual(moves(loaded), [], 'not before the new page has rendered')
    loaded.flush()
    assert.deepEqual(moves(loaded), [{ kind: 'moved' }])
  })

  test('moves in quick succession ask once, and a later move asks again', () => {
    const loaded = load(page('https://example.jp/'))
    loaded.move('https://example.jp/a')
    loaded.move('https://example.jp/b')
    loaded.flush()
    assert.equal(moves(loaded).length, 1)
    loaded.move('https://example.jp/')
    loaded.flush()
    assert.equal(moves(loaded).length, 2)
  })

  test('a navigation that stays on the same address asks nothing', () => {
    const loaded = load(page('https://example.jp/'))
    loaded.move('https://example.jp/')
    loaded.flush()
    loaded.move('https://example.jp/a')
    loaded.move('https://example.jp/')
    loaded.flush()
    assert.deepEqual(moves(loaded), [])
  })

  test('the script listens once however often it runs', () => {
    const loaded = load(page('https://example.jp/'))
    loaded.run()
    loaded.probe(1)
    loaded.probe(2)
    assert.equal(loaded.navigation.listenerCount('navigatesuccess'), 1)
    assert.deepEqual(
      loaded.messages().map((m) => [m.kind, m.id]),
      [
        ['probe', 1],
        ['probe', 2],
      ],
    )
  })

  test("a jump to an anchor asks nothing, but a hash router's route does", () => {
    const loaded = load(page('https://example.jp/docs'))
    loaded.move('https://example.jp/docs#install')
    loaded.flush()
    assert.deepEqual(moves(loaded), [])
    loaded.move('https://example.jp/docs#/guide')
    loaded.flush()
    loaded.move('https://example.jp/docs#!/faq')
    loaded.flush()
    assert.equal(moves(loaded).length, 2)
  })

  test('a route with no leading slash counts too, as in a mail inbox', () => {
    const loaded = load(page('https://mail.example.jp/mail/u/0/#inbox'))
    loaded.move('https://mail.example.jp/mail/u/0/#inbox/FMfcgzQ')
    loaded.flush()
    assert.equal(moves(loaded).length, 1)
  })

  test('a WebKit without the Navigation API still probes', () => {
    const loaded = load(page('https://example.jp/', h('p', {}, '本文の段落である。')), { navigation: undefined })
    loaded.probe(5)
    assert.equal(loaded.messages()[0].sample, '本文の段落である。')
  })
})

describe('sharing with the translator', () => {
  test('each name is declared once, as the files share one closure', () => {
    for (const script of [SCRIPT, TRANSLATOR]) {
      const names = [...script.matchAll(/^(?:(?:async )?function|class|const|let|var) (\w+)/gm)].map((m) => m[1])
      assert.deepEqual(
        names.filter((name, i) => names.indexOf(name) !== i),
        [],
      )
    }
  })

  test('a page element named like the probe does not switch it off', () => {
    const named = { __wisProbe: h('div', { id: '__wisProbe' }) }
    const loaded = load(page('https://example.jp/', h('p', {}, '本文の段落である。')), { named })
    loaded.probe(5)
    assert.equal(loaded.messages()[0].kind, 'probe')
  })
})
