// `translation/page.js`: splits a page into segments, sends the visible ones
// to `wisTranslate`, puts translations in place and restores the original.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { NodeFilter, Text, eventTarget, h, handlers, page } from '../support/s2-dom.mjs'
import { includedFile } from '../support/rustSource.mjs'

const SCRIPT = includedFile('translation/page.js')

/** An IntersectionObserver whose callbacks the test fires by hand. */
function observerClass() {
  const observers = []
  class FakeObserver {
    constructor(callback) {
      this.callback = callback
      this.targets = new Set()
      observers.push(this)
    }
    observe(target) {
      this.targets.add(target)
    }
    unobserve(target) {
      this.targets.delete(target)
    }
    disconnect() {
      this.targets.clear()
    }
    /** Report `targets` as scrolled into view. */
    show(...targets) {
      this.callback(targets.map((target) => ({ target, isIntersecting: true })))
    }
  }
  return { FakeObserver, observers }
}

/** A MutationObserver whose records the test hands over by hand. */
function mutationClass() {
  const mutations = []
  class FakeMutations {
    constructor(callback) {
      this.callback = callback
      this.watching = false
      mutations.push(this)
    }
    observe() {
      this.watching = true
    }
    disconnect() {
      this.watching = false
    }
    takeRecords() {
      return []
    }
    /** Report `records` as the page's changes. */
    fire(...records) {
      if (this.watching) this.callback(records)
    }
  }
  return { FakeMutations, mutations }
}

/**
 * Runs the script over `doc` and returns its API, what it posted, and the
 * observers. `findMain` stands in for the reader's main-content finder.
 */
function load(doc, { findMain } = {}) {
  const { webkit, sent } = handlers('wisTranslate')
  const getComputedStyle = (el) => ({ display: el.style.display || '' })
  const window = eventTarget({ webkit, getComputedStyle })
  const { FakeObserver, observers } = observerClass()
  const { FakeMutations, mutations } = mutationClass()
  const timers = []
  const setTimeout = (fn) => timers.push(fn)
  new Function(
    'window',
    'document',
    'NodeFilter',
    'IntersectionObserver',
    'setTimeout',
    'MutationObserver',
    '__wisFindMainContent',
    SCRIPT,
  )(window, doc, NodeFilter, FakeObserver, setTimeout, FakeMutations, findMain)
  const messages = () => sent.map(({ body }) => JSON.parse(body))
  const flush = () => {
    while (timers.length) timers.shift()()
  }
  return { api: window.__wisTranslate, window, messages, observers, mutations, flush }
}

/**
 * Starts and runs translation generation `gen` from `source`, with every root
 * on screen.
 */
function goVisible(loaded, gen = 1, source = '', target = 'en') {
  loaded.api.start(gen)
  loaded.api.go(gen, source, target)
  const observer = loaded.observers.at(-1)
  observer.show(...observer.targets)
  loaded.flush()
  return loaded.messages().filter((m) => m.kind === 'blocks')
}

const blocksOf = (messages) => messages.flatMap((m) => m.blocks)

describe('the page language', () => {
  test('start reports the language the page declares', () => {
    const doc = page('https://example.jp/', h('p', {}, 'こんにちは'))
    doc.documentElement.setAttribute('lang', 'ja')
    const loaded = load(doc)
    loaded.api.start(1)
    assert.deepEqual(loaded.messages(), [{ kind: 'start', gen: 1, lang: 'ja' }])
  })

  test('a Content-Language meta stands in for a missing lang', () => {
    const doc = page('https://example.jp/', h('p', {}, 'こんにちは'))
    doc.head.appendChild(h('meta', { 'http-equiv': 'Content-Language', content: 'ja-JP' }))
    const loaded = load(doc)
    loaded.api.start(1)
    assert.equal(loaded.messages()[0].lang, 'ja-JP')
  })

  test('an overlong declaration is cut so the check can refuse it plainly', () => {
    const doc = page('https://example.jp/', h('p', {}, 'こんにちは'))
    doc.documentElement.setAttribute('lang', 'ja-' + 'x'.repeat(200))
    const loaded = load(doc)
    loaded.api.start(1)
    assert.equal(loaded.messages()[0].lang.length, 64)
  })

  test('a page that declares nothing reports an empty language', () => {
    const loaded = load(page('https://example.jp/', h('p', {}, 'こんにちは')))
    loaded.api.start(1)
    assert.equal(loaded.messages()[0].lang, '')
  })
})

describe('finding segments', () => {
  test('inline elements are tagged by their place in the segment', () => {
    const doc = page(
      'https://example.jp/',
      h('p', {}, '詳しくは', h('a', { href: '/x' }, 'こちら'), 'から', h('b', {}, '全文'), 'を。'),
    )
    const blocks = blocksOf(goVisible(load(doc)))
    assert.deepEqual(blocks, [
      {
        id: 0,
        runs: [
          { text: '詳しくは' },
          { text: 'こちら', tag: 0 },
          { text: 'から' },
          { text: '全文', tag: 1 },
          { text: 'を。' },
        ],
        main: false,
        needed: [0],
      },
    ])
  })

  test('each block is its own segment, nested inline elements included', () => {
    const doc = page('https://example.jp/', h('h1', {}, '見出し'), h('div', {}, h('p', {}, '一段落'), '続き'))
    const runs = blocksOf(goVisible(load(doc))).map((b) => b.runs.map((r) => r.text).join(''))
    assert.deepEqual(runs, ['見出し', '一段落', '続き'])
  })

  test('code, preformatted text, scripts and opted-out content are left alone', () => {
    const doc = page(
      'https://example.jp/',
      h('p', {}, '本文'),
      h('pre', {}, 'let x = 1'),
      h('p', {}, h('code', {}, 'npm install')),
      h('script', {}, 'var a = 1'),
      h('p', { translate: 'no' }, '固有名詞'),
      h('p', { class: 'notranslate' }, 'ブランド'),
      h('div', { contenteditable: 'true' }, '下書き'),
    )
    const texts = blocksOf(goVisible(load(doc))).map((b) => b.runs.map((r) => r.text).join(''))
    assert.deepEqual(texts, ['本文'])
  })

  test('content marked as another language than the source is left alone', () => {
    const menu = h(
      'ul',
      {},
      h('li', {}, h('a', { href: '/af', lang: 'af' }, 'Afrikaans')),
      h('li', { lang: 'ar' }, 'العربية'),
      h('li', { lang: 'ja-JP' }, '日本語'),
    )
    const doc = page('https://example.jp/', h('p', {}, '本文'), menu)
    doc.documentElement.setAttribute('lang', 'ja')
    const texts = blocksOf(goVisible(load(doc), 1, 'ja')).map((b) => b.runs[0].text)
    assert.deepEqual(texts, ['本文', '日本語'])
  })

  test('a phrase marked as another language stays in its sentence, untranslated', () => {
    const term = h('span', { lang: 'en' }, 'iPhone')
    const p = h('p', {}, 'これは', term, 'です')
    const doc = page('https://example.jp/', p)
    doc.documentElement.setAttribute('lang', 'ja')
    const loaded = load(doc)
    const sent = blocksOf(goVisible(loaded, 1, 'ja')).map((b) => b.runs)
    assert.deepEqual(sent, [[{ text: 'これは' }, { text: 'iPhone', tag: 0 }, { text: 'です' }]])
    loaded.api.apply(1, 0, [{ text: 'This is an ' }, { text: 'iphone', tag: 0 }, { text: '.' }])
    assert.equal(p.innerHTML, 'This is an <span lang="en">iPhone</span>.')
  })

  test('codes for the same language, and codes for none, are not another language', () => {
    const doc = page(
      'https://example.no/',
      h('main', { lang: 'nb' }, h('p', {}, 'Bokmål')),
      h('p', { lang: 'nn' }, 'Nynorsk'),
      h('p', { lang: 'und' }, 'Ukjent'),
      h('p', { lang: 'x-klingon' }, 'Privat'),
      h('p', { lang: 'sv' }, 'Svenska'),
    )
    const texts = blocksOf(goVisible(load(doc), 1, 'no')).map((b) => b.runs[0].text)
    assert.deepEqual(texts, ['Bokmål', 'Nynorsk', 'Ukjent', 'Privat'])
  })

  test('an article marked in the source language is kept on a page marked otherwise', () => {
    const doc = page('https://example.jp/', h('p', {}, 'Menu'), h('article', { lang: 'ja' }, h('p', {}, '本文')))
    doc.documentElement.setAttribute('lang', 'en')
    const texts = blocksOf(goVisible(load(doc), 1, 'ja')).map((b) => b.runs[0].text)
    assert.deepEqual(texts, ['本文'])
  })

  test('elements with no text between them go one by one, and numbers alone stay', () => {
    const entry = h('a', { href: '#s1' }, h('span', {}, '3.1'), '\n  ', h('span', {}, '構造と動作'))
    const doc = page('https://example.jp/', h('ul', {}, h('li', {}, entry)))
    assert.deepEqual(blocksOf(goVisible(load(doc))).map((b) => b.runs), [[{ text: '構造と動作' }]])
  })

  test('a word-break hint or an empty element inside a sentence keeps it whole', () => {
    const p = h(
      'p',
      {},
      '下取りに出すと',
      h('wbr'),
      h('span', { class: 'nowrap' }, '実質'),
      h('wbr'),
      h('span', { class: 'adjust' }),
      h('span', { class: 'nowrap' }, 'お得'),
      'です。',
    )
    const blocks = blocksOf(goVisible(load(page('https://example.jp/', p))))
    assert.deepEqual(
      blocks.map((b) => b.runs),
      [[{ text: '下取りに出すと' }, { text: '実質', tag: 0 }, { text: 'お得', tag: 1 }, { text: 'です。' }]],
    )
    assert.deepEqual(blocks[0].needed, [])
  })

  test('word-break hints go back where they were when a reordered sentence is reverted', () => {
    const p = h('p', {}, '詳しくは', h('wbr'), h('a', { href: '/x' }, 'こちら'), h('wbr'), 'を見る')
    const loaded = load(page('https://example.jp/', p))
    const before = p.innerHTML
    goVisible(loaded)
    loaded.api.apply(1, 0, [{ text: 'here', tag: 0 }, { text: ' for details' }])
    assert.equal(p.textContent, 'here for details')
    loaded.api.revert()
    assert.equal(p.innerHTML, before)
  })

  test('a sentence of styling elements followed by a link keeps the sentence whole', () => {
    const div = h(
      'div',
      {},
      h('span', { class: 'nowrap' }, 'キャリア契約で、'),
      h('wbr'),
      h('span', { class: 'nowrap' }, 'さらに割引です。'),
      h('wbr'),
      h('a', { href: '/buy' }, 'さらに詳しく'),
    )
    const blocks = blocksOf(goVisible(load(page('https://example.jp/', div))))
    assert.deepEqual(
      blocks.map((b) => b.runs.map((r) => r.text).join('')),
      ['キャリア契約で、さらに割引です。', 'さらに詳しく'],
    )
  })

  test('silent elements stay beside the text they followed', () => {
    const footnote = h('sup', {}, h('a', { href: '#f' }, '1'))
    const p = h('p', {}, 'これは', h('a', { href: '/x' }, 'リンク'), h('wbr'), 'です', footnote, '。')
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded)
    loaded.api.apply(1, 0, [{ text: 'This is a ' }, { text: 'link', tag: 0 }, { text: '.' }])
    assert.equal(p.innerHTML, 'This is a <a href="/x">link</a><wbr>.<sup><a href="#f">1</a></sup>')
  })

  test('a styling element holding a link must come back', () => {
    const p = h('p', {}, 'これは', h('span', { class: 'nowrap' }, '詳しくは', h('a', { href: '/x' }, 'こちら')), 'です。')
    const loaded = load(page('https://example.jp/', p))
    const before = p.innerHTML
    const [block] = blocksOf(goVisible(loaded))
    assert.deepEqual(block.needed, [0])
    loaded.api.apply(1, 0, [{ text: 'This is, for details, here.' }])
    assert.equal(p.innerHTML, before)
  })

  test('chemistry and units keep their numbers; furigana is not read', () => {
    const doc = page(
      'https://example.jp/',
      h('p', {}, '二酸化炭素CO', h('sub', {}, '2'), 'は気体です。'),
      h('p', {}, '広さは10m', h('sup', {}, '2'), 'です。'),
      h('p', {}, h('ruby', {}, '漢字', h('rt', {}, 'かんじ')), 'を読む。'),
    )
    const texts = blocksOf(goVisible(load(doc))).map((b) => b.runs.map((r) => r.text).join(''))
    assert.deepEqual(texts, ['二酸化炭素CO2は気体です。', '広さは10m2です。', '漢字を読む。'])
  })

  test('a script or a custom element inside a sentence still ends it', () => {
    const doc = page(
      'https://example.jp/',
      h('p', {}, '前の文', h('script', { src: 'x.js' }), '後の文'),
      h('p', {}, '別の文', h('ion-icon', { name: 'star' }), '最後の文'),
    )
    assert.equal(blocksOf(goVisible(load(doc))).length, 4)
  })

  test('an empty element the page later fills makes the sentence be read again', () => {
    const slot = h('span', { class: 'price' })
    const p = h('p', {}, '価格は', slot, 'です。')
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded)
    const filled = new Text('千円')
    slot.appendChild(filled)
    loaded.mutations.at(-1).fire({ type: 'childList', target: slot, addedNodes: [filled], removedNodes: [] })
    loaded.flush()
    loaded.observers.at(-1).show(...loaded.observers.at(-1).targets)
    loaded.flush()
    const texts = blocksOf(loaded.messages().filter((m) => m.kind === 'blocks')).map((b) =>
      b.runs.map((r) => r.text).join(''),
    )
    assert.deepEqual(texts, ['価格はです。', '価格は千円です。'])
  })

  test('a footnote number stays in place and is not read as part of the sentence', () => {
    const note = () => h('sup', {}, h('a', { href: '#fn' }, '1'))
    const loose = h('p', {}, '36回から', note(), '。24か月目です。')
    const nested = h('p', {}, h('span', { class: 'nowrap' }, '36回から', note(), '。'), 'です。')
    const blocks = blocksOf(goVisible(load(page('https://example.jp/', loose, nested))))
    assert.deepEqual(
      blocks.map((b) => b.runs.map((r) => r.text).join('')),
      ['36回から。24か月目です。', '36回から。です。'],
    )
  })

  test('punctuation given its own styling element stays with the word before it', () => {
    const p = h('p', {}, h('span', { class: 'a' }, 'これは短い'), h('wbr'), h('span', { class: 'b' }, '文です。'))
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded)
    loaded.api.apply(1, 0, [{ text: 'This is a short sentence', tag: 0 }, { text: '.', tag: 1 }])
    assert.equal(p.innerHTML, '<span class="a">This is a short sentence.</span><wbr><span class="b"></span>')
  })

  test('a styling element that comes back twice is plain text the second time', () => {
    const note = h('sup', {}, h('a', { href: '#f3' }, '3'))
    const p = h('p', {}, h('span', { class: 'a' }, '割引に', note, '。'), h('wbr'), h('span', { class: 'b' }, 'さらにお得です。'))
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded)
    loaded.api.apply(1, 0, [{ text: 'You can get ', tag: 0 }, { text: 'more', tag: 1 }, { text: ' discount.', tag: 0 }])
    assert.equal(p.querySelectorAll('sup').length, 1)
    assert.equal(p.textContent, 'You can get 3more discount.')
  })

  test('punctuation at the start of a styling element moves to the text before it', () => {
    const loose = h('p', {}, '指先で', h('wbr'), h('span', { class: 'nowrap' }, '本能のままに。'))
    const styled = h('p', {}, h('span', { class: 'a' }, '指先で'), h('wbr'), h('span', { class: 'b' }, '本能のままに。'))
    const loaded = load(page('https://example.jp/', loose, styled))
    const sent = blocksOf(goVisible(loaded))
    loaded.api.apply(1, sent[0].id, [{ text: 'With the tips of your fingers' }, { text: ', as instinct dictates.', tag: 0 }])
    loaded.api.apply(1, sent[1].id, [
      { text: 'With your fingertips', tag: 0 },
      { text: ', by instinct.', tag: 1 },
    ])
    assert.equal(loose.innerHTML, 'With the tips of your fingers, <wbr><span class="nowrap">as instinct dictates.</span>')
    assert.equal(styled.innerHTML, '<span class="a">With your fingertips,</span><wbr> <span class="b">by instinct.</span>')
  })

  test('a heading shown inline before its paragraph gets a space after it', () => {
    const heading = h('h3', {}, 'タンデムOLED。')
    const body = h('p', {}, 'PROLED XDRは2層です。')
    heading.style.display = 'inline'
    body.style.display = 'inline'
    const card = h('div', {}, heading, '\u200b', body)
    const plain = h('div', {}, h('h3', {}, '見出し'), h('p', {}, '本文です。'))
    const loaded = load(page('https://example.jp/', card, plain))
    const sent = blocksOf(goVisible(loaded))
    const id = (text) => sent.find((b) => b.runs[0].text === text).id
    loaded.api.apply(1, id('タンデムOLED。'), [{ text: 'Tandem OLED.' }])
    loaded.api.apply(1, id('PROLED XDRは2層です。'), [{ text: 'PROLED XDR has two layers.' }])
    loaded.api.apply(1, id('本文です。'), [{ text: 'This is the body.' }])
    assert.equal(body.textContent, ' PROLED XDR has two layers.')
    assert.equal(plain.childNodes[1].textContent, 'This is the body.')
  })

  test('punctuation after a link is not pulled into it', () => {
    const p = h('p', {}, '詳しくは', h('a', { href: '/x' }, 'こちら'), '。')
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded)
    loaded.api.apply(1, 0, [{ text: 'For details, see ' }, { text: 'here', tag: 0 }, { text: '.' }])
    assert.equal(p.innerHTML, 'For details, see <a href="/x">here</a>.')
  })

  test('a segment right after other text gets a space in English, not after a line break', () => {
    const link = h('a', { href: '/buy' }, 'さらに詳しく')
    const div = h('div', {}, h('span', { class: 'nowrap' }, '割引です。'), link)
    const lines = h('p', {}, '一行目', h('br'), '二行目')
    const loaded = load(page('https://example.jp/', div, lines))
    const before = div.innerHTML
    const sent = blocksOf(goVisible(loaded))
    const id = (text) => sent.find((b) => b.runs[0].text === text).id
    loaded.api.apply(1, id('さらに詳しく'), [{ text: 'Learn more' }])
    loaded.api.apply(1, id('二行目'), [{ text: 'Line two' }])
    assert.equal(div.innerHTML, '<span class="nowrap">割引です。</span> <a href="/buy">Learn more</a>')
    assert.equal(lines.innerHTML, '一行目<br>Line two')
    loaded.api.revert()
    assert.equal(div.innerHTML, before)
  })

  test('no space goes in after a line break or an opening bracket inside the text before', () => {
    const afterBreak = h('a', { href: '/a' }, 'もっと')
    const afterBracket = h('a', { href: '/b' }, '詳細')
    const one = h('div', {}, h('span', {}, '一行目', h('br')), afterBreak)
    const two = h('div', {}, h('span', {}, '【'), afterBracket)
    const loaded = load(page('https://example.jp/', one, two))
    const sent = blocksOf(goVisible(loaded))
    const id = (text) => sent.find((b) => b.runs[0].text === text).id
    loaded.api.apply(1, id('もっと'), [{ text: 'More' }])
    loaded.api.apply(1, id('詳細'), [{ text: 'Details' }])
    assert.equal(one.innerHTML, '<span>一行目<br></span><a href="/a">More</a>')
    assert.equal(two.innerHTML, '<span>【</span><a href="/b">Details</a>')
  })

  test('a gap before a translation that starts with a link reverts exactly', () => {
    const p = h('p', {}, h('b', {}, '注意'), h('a', { href: '/x' }, 'こちら'), 'を見る')
    const loaded = load(page('https://example.jp/', p))
    const before = p.innerHTML
    const sent = blocksOf(goVisible(loaded))
    assert.equal(sent.length, 1)
    loaded.api.apply(1, sent[0].id, [{ text: 'Note', tag: 0 }, { text: ' see ' }, { text: 'here', tag: 1 }])
    assert.equal(p.textContent, 'Note see here')
    loaded.api.revert()
    assert.equal(p.innerHTML, before)
  })

  test('a language written without spaces gets no space between segments', () => {
    const link = h('a', { href: '/buy' }, 'More')
    const div = h('div', {}, h('span', {}, 'A discount.'), link)
    const loaded = load(page('https://example.com/', div))
    const sent = blocksOf(goVisible(loaded, 1, 'en', 'ja'))
    loaded.api.apply(1, sent.find((b) => b.runs[0].text === 'More').id, [{ text: '詳しく' }])
    assert.equal(div.innerHTML, '<span>A discount.</span><a href="/buy">詳しく</a>')
  })

  test('an empty element at the start of a sentence stays first', () => {
    const swatch = h('span', { class: 'legend-color' }, '\u00a0')
    const legend = h('div', {}, swatch, h('a', { href: '/chrome' }, 'Google Chrome'), ' (64.7%)')
    const loaded = load(page('https://example.jp/', legend))
    goVisible(loaded)
    loaded.api.apply(1, 0, [{ text: '(64.7%) ' }, { text: 'Google Chrome', tag: 0 }])
    assert.equal(legend.textContent, '\u00a0(64.7%) Google Chrome')
    assert.equal(legend.childNodes[0], swatch)
  })

  test('a sentence made only of styling elements stays whole', () => {
    const p = h('p', {}, h('span', { class: 'nowrap' }, 'iPhoneで、'), h('span', { class: 'nowrap' }, 'もっと楽しく。'))
    const loaded = load(page('https://example.jp/', p))
    assert.equal(blocksOf(goVisible(loaded)).length, 1)
    loaded.api.apply(1, 0, [{ text: 'Have more fun with iPhone.' }])
    assert.equal(p.textContent, 'Have more fun with iPhone.')
  })

  test('a segment that is one link is sent whole, untagged', () => {
    const doc = page('https://example.jp/', h('ul', {}, h('li', {}, h('a', { href: '/' }, 'ホーム'))))
    assert.deepEqual(blocksOf(goVisible(load(doc))), [{ id: 0, runs: [{ text: 'ホーム' }], main: false, needed: [] }])
  })

  test('a line break or an image splits a block into segments', () => {
    const doc = page(
      'https://example.jp/',
      h('p', {}, '一行目', h('br'), '二行目', h('img', { src: '/i.png' }), '三行目'),
    )
    const texts = blocksOf(goVisible(load(doc))).map((b) => b.runs.map((r) => r.text).join(''))
    assert.deepEqual(texts, ['一行目', '二行目', '三行目'])
  })

  test('a segment with more pieces than a message may carry is left as it is', () => {
    const pieces = []
    for (let i = 0; i < 70; i++) pieces.push('語', h('a', { href: `/${i}` }, `リンク${i}`))
    const doc = page('https://example.jp/', h('p', {}, ...pieces), h('p', {}, '短い段落'))
    const texts = blocksOf(goVisible(load(doc))).map((b) => b.runs.map((r) => r.text).join(''))
    assert.deepEqual(texts, ['短い段落'])
  })

  test('blank text and whitespace between blocks are not sent', () => {
    const doc = page('https://example.jp/', h('div', {}, '  ', h('p', {}, '本文'), '\n'))
    const texts = blocksOf(goVisible(load(doc))).map((b) => b.runs.map((r) => r.text).join(''))
    assert.deepEqual(texts, ['本文'])
  })
})

describe('sending what is on screen', () => {
  test('nothing is sent until a block scrolls into view', () => {
    const first = h('p', {}, '一つ目')
    const second = h('p', {}, '二つ目')
    const loaded = load(page('https://example.jp/', first, second))
    loaded.api.start(1)
    loaded.api.go(1)
    loaded.flush()
    assert.deepEqual(blocksOf(loaded.messages().filter((m) => m.kind === 'blocks')), [])

    loaded.observers[0].show(second)
    loaded.flush()
    const sent = blocksOf(loaded.messages().filter((m) => m.kind === 'blocks'))
    assert.deepEqual(sent, [{ id: 1, runs: [{ text: '二つ目' }], main: false, needed: [] }])
    assert.ok(!loaded.observers[0].targets.has(second), 'a sent block stops being watched')
  })

  test('a long page goes out in batches tagged with its generation', () => {
    const paragraphs = Array.from({ length: 50 }, (_, i) => h('p', {}, `段落${i}`))
    const messages = goVisible(load(page('https://example.jp/', ...paragraphs)), 7)
    assert.ok(messages.length > 1, 'more than one message')
    assert.ok(messages.every((m) => m.gen === 7 && m.wave === 1 && m.blocks.length <= 24))
    assert.deepEqual(
      blocksOf(messages).map((b) => b.id),
      Array.from({ length: 50 }, (_, i) => i),
    )
  })

  test('each scroll that brings segments into view is a new wave', () => {
    const first = h('p', {}, '一つ目')
    const second = h('p', {}, '二つ目')
    const loaded = load(page('https://example.jp/', first, second))
    loaded.api.start(1)
    loaded.api.go(1)
    loaded.observers[0].show(first)
    loaded.flush()
    loaded.observers[0].show(second)
    loaded.flush()
    const waves = loaded.messages().filter((m) => m.kind === 'blocks').map((m) => m.wave)
    assert.deepEqual(waves, [1, 2])
  })

  test('the main content goes before menus that came into view with it', () => {
    const menu = h('nav', {}, h('ul', {}, h('li', {}, 'ホーム'), h('li', {}, 'ヘルプ')))
    const article = h('main', {}, h('p', {}, '本文一'), h('p', {}, '本文二'))
    const loaded = load(page('https://example.jp/', menu, article), { findMain: () => article })
    const sent = blocksOf(goVisible(loaded))
    assert.deepEqual(
      sent.map((b) => [b.runs[0].text, b.main]),
      [['本文一', true], ['本文二', true], ['ホーム', false], ['ヘルプ', false]],
    )
  })

  test('main content stays first when a wave spans several messages', () => {
    const menu = h('nav', {}, h('ul', {}, ...Array.from({ length: 30 }, (_, i) => h('li', {}, `項目${i}`))))
    const article = h('main', {}, h('p', {}, '本文一'), h('p', {}, '本文二'))
    const loaded = load(page('https://example.jp/', menu, article), { findMain: () => article })
    const messages = goVisible(loaded)
    assert.ok(messages.length > 1)
    assert.deepEqual(
      messages[0].blocks.slice(0, 2).map((b) => b.runs[0].text),
      ['本文一', '本文二'],
    )
  })

  test('a page with nothing to send uses no wave', () => {
    const loaded = load(page('https://example.jp/', h('p', {}, '一つ目'), h('p', {}, '二つ目')))
    loaded.api.start(1)
    loaded.api.go(1, '')
    loaded.observers[0].show()
    loaded.flush()
    loaded.observers[0].show(...loaded.observers[0].targets)
    loaded.flush()
    const waves = loaded.messages().filter((m) => m.kind === 'blocks').map((m) => m.wave)
    assert.deepEqual(waves, [1])
  })

  test('a page whose main content cannot be found keeps document order', () => {
    const menu = h('nav', {}, h('ul', {}, h('li', {}, 'ホーム')))
    const article = h('main', {}, h('p', {}, '本文'))
    const findMain = () => {
      throw new TypeError('document.querySelectorAll is not a function')
    }
    const loaded = load(page('https://example.jp/', menu, article), { findMain })
    const texts = blocksOf(goVisible(loaded)).map((b) => b.runs[0].text)
    assert.deepEqual(texts, ['ホーム', '本文'])
  })
})

describe('putting translations in place', () => {
  function translatedParagraph() {
    const link = h('a', { href: '/x' }, 'こちら')
    const bold = h('b', {}, '全文')
    const p = h('p', {}, '詳しくは', link, 'から', bold, 'を。')
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded)
    return { loaded, p, link, bold, before: p.innerHTML }
  }

  test('a translation keeps the same link and bold elements, reordered', () => {
    const { loaded, p, link, bold } = translatedParagraph()
    loaded.api.apply(1, 0, [
      { text: 'For more, read ' },
      { text: 'the full text', tag: 1 },
      { text: ' from ' },
      { text: 'here', tag: 0 },
      { text: '.' },
    ])
    assert.equal(p.textContent, 'For more, read the full text from here.')
    assert.equal(p.innerHTML, 'For more, read <b>the full text</b> from <a href="/x">here</a>.')
    assert.equal(p.childNodes[1], bold)
    assert.equal(p.childNodes[3], link)
  })

  test('revert restores the original nodes and text exactly', () => {
    const { loaded, p, link, before } = translatedParagraph()
    loaded.api.apply(1, 0, [{ text: 'Read ' }, { text: 'all', tag: 1 }, { text: ' ' }, { text: 'here', tag: 0 }])
    loaded.api.revert()
    assert.equal(p.innerHTML, before)
    assert.equal(p.childNodes[1], link)
  })

  test('a translation that dropped a link leaves the segment as it was', () => {
    const { loaded, p, before } = translatedParagraph()
    loaded.api.apply(1, 0, [{ text: 'Read ' }, { text: 'the full text', tag: 1 }, { text: ' here' }])
    assert.equal(p.innerHTML, before)
  })

  test('a styling element the translation dropped is emptied, its words kept in the sentence', () => {
    const p = h('p', {}, 'これは', h('span', { class: 'nowrap' }, '短い'), '文です。')
    const loaded = load(page('https://example.jp/', p))
    const before = p.innerHTML
    goVisible(loaded)
    loaded.api.apply(1, 0, [{ text: 'This is a short sentence.' }])
    assert.equal(p.innerHTML, 'This is a short sentence.<span class="nowrap"></span>')
    loaded.api.revert()
    assert.equal(p.innerHTML, before)
  })

  test('a tag that comes back twice gets a copy of its element', () => {
    const { loaded, p, link } = translatedParagraph()
    loaded.api.apply(1, 0, [
      { text: 'see', tag: 0 },
      { text: ' the ' },
      { text: 'full', tag: 1 },
      { text: ' text, ' },
      { text: 'here', tag: 0 },
    ])
    assert.equal(p.innerHTML, '<a href="/x">see</a> the <b>full</b> text, <a href="/x">here</a>')
    assert.equal(p.childNodes[0], link)
    assert.notEqual(p.childNodes[4], link)
  })

  test('a whole-link segment takes its translation inside the link', () => {
    const link = h('a', { href: '/' }, 'ホーム')
    const li = h('li', {}, link)
    const loaded = load(page('https://example.jp/', h('ul', {}, li)))
    goVisible(loaded)
    loaded.api.apply(1, 0, [{ text: 'Home' }])
    assert.equal(li.innerHTML, '<a href="/">Home</a>')
    loaded.api.revert()
    assert.equal(li.innerHTML, '<a href="/">ホーム</a>')
  })

  test('an element with nested markup keeps it, its first text takes the translation', () => {
    const icon = h('i', { class: 'icon' })
    const link = h('a', { href: '/s' }, icon, ' 設定')
    const p = h('p', {}, '開く: ', link)
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded)
    loaded.api.apply(1, 0, [{ text: 'Open: ' }, { text: 'Settings', tag: 0 }])
    assert.equal(p.innerHTML, 'Open: <a href="/s"><i class="icon"></i>Settings</a>')
    assert.equal(link.childNodes[0], icon)
  })

  test('a stale generation or an unknown segment changes nothing', () => {
    const { loaded, p, before } = translatedParagraph()
    loaded.api.apply(2, 0, [{ text: 'stale' }])
    loaded.api.apply(1, 99, [{ text: 'nowhere' }])
    loaded.api.apply(1, 0, [{ text: 'bad tag', tag: 5 }])
    assert.equal(p.innerHTML, before)
  })

  test('starting again reverts the previous translation first', () => {
    const { loaded, p, before } = translatedParagraph()
    loaded.api.apply(1, 0, [{ text: 'Read ' }, { text: 'all', tag: 1 }, { text: ' ' }, { text: 'here', tag: 0 }])
    loaded.api.start(2)
    assert.equal(p.innerHTML, before)
    loaded.api.go(1)
    loaded.flush()
    assert.equal(loaded.observers.length, 1, 'a go for an old generation does nothing')
  })

  test('an inline element holding a nested block carries only its own text', () => {
    const item = h('li', {}, '項目', h('b', {}, '強調'))
    const span = h('span', {}, '中身', h('ul', {}, item))
    const div = h('div', {}, '前文', span, '後文')
    const loaded = load(page('https://example.jp/', div))
    const blocks = blocksOf(goVisible(loaded))
    assert.deepEqual(
      blocks.map((b) => b.runs),
      [
        [{ text: '前文' }, { text: '中身', tag: 0 }, { text: '後文' }],
        [{ text: '項目' }, { text: '強調', tag: 0 }],
      ],
    )
    const before = div.innerHTML
    loaded.api.apply(1, 1, [{ text: 'Item ' }, { text: 'bold', tag: 0 }])
    loaded.api.apply(1, 0, [{ text: 'Before ' }, { text: 'inside', tag: 0 }, { text: ' after' }])
    assert.equal(item.textContent, 'Item bold')
    assert.equal(span.childNodes[0].nodeValue, 'inside')
    loaded.api.revert()
    assert.equal(div.innerHTML, before)
  })

  test('the page keeps its own text nodes, attached and carrying the translation', () => {
    const { loaded, p } = translatedParagraph()
    const [first, , middle, , last] = p.childNodes
    loaded.api.apply(1, 0, [
      { text: 'For more, read ' },
      { text: 'the full text', tag: 1 },
      { text: ' from ' },
      { text: 'here', tag: 0 },
      { text: '.' },
    ])
    for (const node of [first, middle, last]) assert.equal(node.parentNode, p)
    assert.deepEqual([first.nodeValue, middle.nodeValue, last.nodeValue], ['For more, read ', ' from ', '.'])
    assert.equal(p.textContent, 'For more, read the full text from here.')
  })

  test('more text runs than the page had get new nodes, fewer leave the rest empty', () => {
    const { loaded, p } = translatedParagraph()
    loaded.api.apply(1, 0, [
      { text: 'A ' },
      { text: 'x', tag: 0 },
      { text: ' B ' },
      { text: 'y', tag: 1 },
      { text: ' C' },
      { text: ' D' },
    ])
    assert.equal(p.textContent, 'A x B y C D')
    const { loaded: again, p: q, before } = translatedParagraph()
    again.api.apply(1, 0, [{ text: 'x', tag: 0 }, { text: 'y', tag: 1 }])
    assert.equal(q.textContent, 'xy')
    again.api.revert()
    assert.equal(q.innerHTML, before)
  })

  test('a blank translation of real text is refused', () => {
    const p = h('p', {}, 'こんにちは')
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded)
    loaded.api.apply(1, 0, [])
    loaded.api.apply(1, 0, [{ text: '  ' }])
    assert.equal(p.textContent, 'こんにちは')
  })

  test('a segment whose text changed since it was sent is skipped', () => {
    const p = h('p', {}, 'コメント3件')
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded)
    p.childNodes[0].nodeValue = 'コメント4件'
    loaded.api.apply(1, 0, [{ text: '3 comments' }])
    assert.equal(p.textContent, 'コメント4件')
  })

  test('code inside a sentence stays in it, untranslated', () => {
    const code = h('code', {}, 'ls')
    const p = h('p', {}, '一覧は', code, 'で表示')
    const loaded = load(page('https://example.jp/', p))
    const blocks = blocksOf(goVisible(loaded))
    assert.deepEqual(blocks, [
      { id: 0, runs: [{ text: '一覧は' }, { text: 'ls', tag: 0 }, { text: 'で表示' }], main: false, needed: [0] },
    ])
    loaded.api.apply(1, 0, [{ text: 'Show the list with ' }, { text: 'エルエス', tag: 0 }])
    assert.equal(p.innerHTML, 'Show the list with <code>ls</code>')
    assert.equal(p.childNodes[1], code)
  })

  test('content marked not editable is still translated', () => {
    const doc = page('https://example.jp/', h('div', { contenteditable: 'false' }, '表示のみ'))
    assert.equal(blocksOf(goVisible(load(doc))).length, 1)
  })

  test('a copy made for a repeated tag drops the id', () => {
    const link = h('a', { href: '/x', id: 'more' }, 'こちら')
    const p = h('p', {}, '詳しくは', link)
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded)
    loaded.api.apply(1, 0, [{ text: 'see', tag: 0 }, { text: ' or ' }, { text: 'here', tag: 0 }])
    assert.equal(p.innerHTML, '<a href="/x" id="more">see</a> or <a href="/x">here</a>')
  })

  test('a root reported twice in one batch is sent once', () => {
    const p = h('p', {}, '一度だけ')
    const loaded = load(page('https://example.jp/', p))
    loaded.api.start(1)
    loaded.api.go(1)
    loaded.observers[0].show(p, p)
    loaded.flush()
    assert.equal(blocksOf(loaded.messages().filter((m) => m.kind === 'blocks')).length, 1)
  })

  test('a page put in the back-forward cache is restored as it goes', () => {
    const { loaded, p, before } = translatedParagraph()
    loaded.api.apply(1, 0, [{ text: 'Read ' }, { text: 'all', tag: 1 }, { text: ' ' }, { text: 'here', tag: 0 }])
    loaded.window.fire('pagehide', { persisted: false })
    assert.notEqual(p.innerHTML, before, 'a plain unload leaves the page alone')
    loaded.window.fire('pagehide', { persisted: true })
    assert.equal(p.innerHTML, before)
  })

  test('the spaces at a segment\'s edges survive a translation that drops them', () => {
    const p = h('p', {}, '前の文 ', h('img', { src: '/i.png' }), ' 後の文')
    const loaded = load(page('https://example.jp/', p))
    const sent = blocksOf(goVisible(loaded)).map((b) => b.runs)
    assert.deepEqual(sent, [[{ text: '前の文 ' }], [{ text: ' 後の文' }]])
    loaded.api.apply(1, 0, [{ text: 'Before' }])
    loaded.api.apply(1, 1, [{ text: ' After' }])
    assert.equal(p.innerHTML, 'Before <img src="/i.png"> After')
  })

  test('a lost space before a link goes in front of it in a new node, and reverts', () => {
    const link = h('a', { href: '/x' }, ' こちら')
    const p = h('p', {}, h('img', { src: '/i.png' }), link, 'を見る')
    const rest = p.childNodes[2]
    const loaded = load(page('https://example.jp/', p))
    const before = p.innerHTML
    goVisible(loaded)
    loaded.api.apply(1, 0, [{ text: 'here', tag: 0 }, { text: ' to see' }])
    assert.equal(p.innerHTML, '<img src="/i.png"> <a href="/x">here</a> to see')
    assert.equal(p.childNodes[2], link)
    assert.equal(p.childNodes[3], rest, 'the page\'s text node keeps its place after the link')
    assert.equal(rest.nodeValue, ' to see')
    loaded.api.revert()
    assert.equal(p.innerHTML, before)
  })

  test('a lost space after a link goes behind it', () => {
    const p = h('p', {}, '見る', h('a', { href: '/x' }, 'こちら '), h('img', { src: '/i.png' }))
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded)
    loaded.api.apply(1, 0, [{ text: 'See ' }, { text: 'here', tag: 0 }])
    assert.equal(p.innerHTML, 'See <a href="/x">here</a> <img src="/i.png">')
  })

  test('a whole-link segment keeps the spaces inside its link', () => {
    const link = h('a', { href: '/' }, ' ホーム ')
    const loaded = load(page('https://example.jp/', h('ul', {}, h('li', {}, link))))
    goVisible(loaded)
    loaded.api.apply(1, 0, [{ text: 'Home' }])
    assert.equal(link.textContent, ' Home ')
  })

  test('a segment the page repeats is sent once and every copy filled together', () => {
    const edit = (href) => h('span', { class: 'mw-editsection' }, '[', h('a', { href }, '編集'), ']')
    const first = edit('/e1')
    const later = edit('/e2')
    const loaded = load(page('https://example.jp/', h('div', {}, first), h('div', {}, later)))
    loaded.api.start(1)
    loaded.api.go(1, '')
    loaded.observers[0].show(first)
    loaded.flush()
    const sent = blocksOf(loaded.messages().filter((m) => m.kind === 'blocks'))
    assert.equal(sent.length, 1)
    loaded.api.apply(1, sent[0].id, [{ text: '[' }, { text: 'edit', tag: 0 }, { text: ']' }])
    assert.equal(first.innerHTML, '[<a href="/e1">edit</a>]')
    assert.equal(later.innerHTML, '[<a href="/e2">edit</a>]', 'a copy not yet on screen is filled too')
    loaded.observers[0].show(later)
    loaded.flush()
    assert.equal(blocksOf(loaded.messages().filter((m) => m.kind === 'blocks')).length, 1)
    loaded.api.revert()
    assert.equal(later.innerHTML, '[<a href="/e2">編集</a>]')
  })

  test('two copies on screen together are sent once and both filled', () => {
    const one = h('p', {}, '返信')
    const two = h('p', {}, '返信')
    const loaded = load(page('https://example.jp/', one, two))
    goVisible(loaded)
    const sent = blocksOf(loaded.messages().filter((m) => m.kind === 'blocks'))
    assert.equal(sent.length, 1)
    loaded.api.apply(1, sent[0].id, [{ text: 'Reply' }])
    assert.deepEqual([one.textContent, two.textContent], ['Reply', 'Reply'])
  })

  test('copies that differ in what stays untranslated are sent apart', () => {
    const doc = page(
      'https://example.jp/',
      h('p', {}, 'これは', h('span', { lang: 'en' }, 'Python'), 'です'),
      h('p', {}, 'これは', h('span', {}, 'Python'), 'です'),
    )
    doc.documentElement.setAttribute('lang', 'ja')
    assert.equal(blocksOf(goVisible(load(doc), 1, 'ja')).length, 2)
  })

  test('a segment the page has since changed is skipped, not forced', () => {
    const { loaded, p } = translatedParagraph()
    p.childNodes[0].remove()
    p.insertBefore(new Text('置き換え'), p.childNodes[0])
    const changed = p.innerHTML
    loaded.api.apply(1, 0, [{ text: 'For more ' }, { text: 'text', tag: 1 }, { text: ' ' }, { text: 'here', tag: 0 }])
    assert.equal(p.innerHTML, changed)
  })
})

describe('turning translation back on', () => {
  function translatedTwice() {
    const first = h('p', {}, '一つ目')
    const second = h('p', {}, '二つ目')
    const loaded = load(page('https://example.jp/', first, second))
    loaded.api.start(1)
    loaded.api.go(1, 'ja', 'en')
    loaded.observers[0].show(first)
    loaded.flush()
    loaded.api.apply(1, 0, [{ text: 'First' }])
    loaded.api.revert()
    return { loaded, first, second }
  }

  const sentFor = (loaded, gen) => blocksOf(loaded.messages().filter((m) => m.kind === 'blocks' && m.gen === gen))

  test('puts the translations it had straight back, without asking again', () => {
    const { loaded, first } = translatedTwice()
    assert.equal(first.textContent, '一つ目')
    loaded.api.start(2)
    loaded.api.go(2, 'ja', 'en')
    assert.equal(first.textContent, 'First', 'back before anything scrolls into view')
    loaded.observers.at(-1).show(...loaded.observers.at(-1).targets)
    loaded.flush()
    assert.deepEqual(sentFor(loaded, 2).map((b) => b.runs[0].text), ['二つ目'])
  })

  test('remembers translations for one language pair only', () => {
    const { loaded, first } = translatedTwice()
    loaded.api.start(2)
    loaded.api.go(2, 'ja', 'fr')
    assert.equal(first.textContent, '一つ目')
  })

  test('a segment the page has changed since is sent again', () => {
    const { loaded, first } = translatedTwice()
    first.childNodes[0].nodeValue = '新しい一つ目'
    loaded.api.start(2)
    loaded.api.go(2, 'ja', 'en')
    assert.equal(first.textContent, '新しい一つ目')
    loaded.observers.at(-1).show(...loaded.observers.at(-1).targets)
    loaded.flush()
    assert.deepEqual(sentFor(loaded, 2).map((b) => b.runs[0].text), ['新しい一つ目', '二つ目'])
  })

  test('a long paragraph and every copy of a repeated one come back', () => {
    const long = h('p', {}, 'あ'.repeat(300))
    const one = h('p', {}, '返信')
    const two = h('p', {}, '返信')
    const loaded = load(page('https://example.jp/', long, one, two))
    goVisible(loaded, 1, 'ja')
    loaded.api.apply(1, 0, [{ text: 'A'.repeat(100) }])
    loaded.api.apply(1, 1, [{ text: 'Reply' }])
    loaded.api.revert()
    loaded.api.start(2)
    loaded.api.go(2, 'ja', 'en')
    assert.deepEqual([long.textContent, one.textContent, two.textContent], ['A'.repeat(100), 'Reply', 'Reply'])
  })

  test('switching to another language pair and back translates afresh', () => {
    const { loaded, first } = translatedTwice()
    loaded.api.start(2)
    loaded.api.go(2, 'ja', 'fr')
    loaded.api.revert()
    loaded.api.start(3)
    loaded.api.go(3, 'ja', 'en')
    assert.equal(first.textContent, '一つ目')
  })

  test('a restore that fails leaves the page watched and revertible', () => {
    const link = h('a', { href: '/x' }, 'こちら')
    const p = h('p', {}, '詳しくは', link, 'を。')
    const other = h('p', {}, '別の段落')
    const loaded = load(page('https://example.jp/', p, other))
    const before = p.innerHTML
    goVisible(loaded, 1, 'ja')
    loaded.api.apply(1, 0, [{ text: 'here', tag: 0 }, { text: ' for details.' }])
    loaded.api.revert()
    p.insertBefore = () => {
      throw new Error('the page broke insertBefore')
    }
    loaded.api.start(2)
    loaded.api.go(2, 'ja', 'en')
    loaded.observers.at(-1).show(...loaded.observers.at(-1).targets)
    loaded.flush()
    assert.deepEqual(sentFor(loaded, 2).map((b) => b.runs[0].text), ['別の段落'])
    loaded.api.revert()
    assert.equal(p.innerHTML, before)
  })

  test('a translation the page refused is not remembered', () => {
    const link = h('a', { href: '/x' }, 'こちら')
    const p = h('p', {}, '詳しくは', link, 'を。')
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded, 1, 'ja')
    loaded.api.apply(1, 0, [{ text: 'See this.' }])
    loaded.api.revert()
    loaded.api.start(2)
    loaded.api.go(2, 'ja', 'en')
    assert.equal(p.textContent, '詳しくはこちらを。')
  })
})

describe('a page that changes after translating', () => {
  const sentTexts = (loaded) =>
    blocksOf(loaded.messages().filter((m) => m.kind === 'blocks')).map((b) => b.runs[0].text)
  const added = (target, ...nodes) => ({ type: 'childList', target, addedNodes: nodes, removedNodes: [] })

  test('text the page adds later is sent when it comes into view', () => {
    const list = h('ul', {}, h('li', {}, '一つ目'))
    const loaded = load(page('https://example.jp/', list))
    goVisible(loaded, 1, 'ja')
    const item = h('li', {}, '二つ目')
    list.appendChild(item)
    loaded.mutations.at(-1).fire(added(list, item))
    loaded.flush()
    loaded.observers.at(-1).show(item)
    loaded.flush()
    assert.deepEqual(sentTexts(loaded), ['一つ目', '二つ目'])
  })

  test('text the page re-renders as it was comes back translated at once', () => {
    const p = h('p', {}, '一つ目')
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded, 1, 'ja')
    loaded.api.apply(1, 0, [{ text: 'First' }])
    p.removeChild(p.childNodes[0])
    const fresh = new Text('一つ目')
    p.appendChild(fresh)
    loaded.mutations.at(-1).fire(added(p, fresh))
    loaded.flush()
    assert.equal(p.textContent, 'First')
    assert.deepEqual(sentTexts(loaded), ['一つ目'])
  })

  test('new text the page writes over a translation is sent in turn', () => {
    const p = h('p', {}, '一つ目')
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded, 1, 'ja')
    loaded.api.apply(1, 0, [{ text: 'First' }])
    p.childNodes[0].nodeValue = '三件の返信'
    loaded.mutations.at(-1).fire({ type: 'characterData', target: p.childNodes[0] })
    loaded.flush()
    assert.equal(p.textContent, '三件の返信')
    loaded.observers.at(-1).show(p)
    loaded.flush()
    assert.deepEqual(sentTexts(loaded), ['一つ目', '三件の返信'])
  })

  test('a rescan leaves the nodes Wis made for a translation alone', () => {
    const link = h('a', { href: '/x' }, 'こちら')
    const p = h('p', {}, '詳しくは', link)
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded, 1, 'ja')
    loaded.api.apply(1, 0, [{ text: 'See ' }, { text: 'here', tag: 0 }, { text: ' and ' }, { text: 'here', tag: 0 }])
    const note = h('span', {}, '追記')
    p.appendChild(note)
    loaded.mutations.at(-1).fire(added(p, note))
    loaded.flush()
    loaded.observers.at(-1).show(...loaded.observers.at(-1).targets)
    loaded.flush()
    assert.deepEqual(sentTexts(loaded), ['詳しくは', '追記'])
  })

  const rewritten = (node, value) => {
    node.nodeValue = value
    return { type: 'characterData', target: node }
  }

  test('a reply for text the page changed meanwhile still fills its later copies', () => {
    const like = h('button', {}, 'いいね')
    const loaded = load(page('https://example.jp/', like))
    goVisible(loaded, 1, 'ja')
    const text = like.childNodes[0]
    loaded.mutations.at(-1).fire(rewritten(text, 'いいね済み'))
    loaded.flush()
    loaded.api.apply(1, 0, [{ text: 'Like' }])
    loaded.mutations.at(-1).fire(rewritten(text, 'いいね'))
    loaded.flush()
    assert.equal(like.textContent, 'Like')
  })

  test('segments the page throws away stop counting toward the cap', () => {
    const items = Array.from({ length: 10000 }, (_, i) => h('li', {}, `項目${i}`))
    const list = h('ul', {}, ...items)
    const loaded = load(page('https://example.jp/', list))
    goVisible(loaded, 1, 'ja')
    const gone = items.slice(0, 10)
    for (const item of gone) list.removeChild(item)
    loaded.mutations.at(-1).fire({ type: 'childList', target: list, addedNodes: [], removedNodes: gone })
    loaded.flush()
    const item = h('li', {}, '追加')
    list.appendChild(item)
    loaded.mutations.at(-1).fire(added(list, item))
    loaded.flush()
    loaded.observers.at(-1).show(item)
    loaded.flush()
    assert.equal(sentTexts(loaded).at(-1), '追加')
  })

  test('a node the page keeps rewriting is left to the page', () => {
    const p = h('p', {}, '一つ目')
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded, 1, 'ja')
    loaded.api.apply(1, 0, [{ text: 'First' }])
    const text = p.childNodes[0]
    for (let i = 0; i < 5; i++) {
      loaded.mutations.at(-1).fire(rewritten(text, '一つ目'))
      loaded.flush()
    }
    assert.equal(p.textContent, '一つ目')
  })

  test('dropping a segment takes away the nodes Wis made for it', () => {
    const link = h('a', { href: '/x' }, 'こちら')
    const p = h('p', {}, '詳しくは', link)
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded, 1, 'ja')
    loaded.api.apply(1, 0, [{ text: 'See ' }, { text: 'here', tag: 0 }, { text: ' and ' }, { text: 'here', tag: 0 }])
    const first = p.childNodes.find((n) => n.nodeType === 3 && n.nodeValue === 'See ')
    loaded.mutations.at(-1).fire(rewritten(first, '新しい'))
    loaded.flush()
    assert.equal(p.innerHTML, '新しい<a href="/x">こちら</a>')
    loaded.observers.at(-1).show(...loaded.observers.at(-1).targets)
    loaded.flush()
    assert.deepEqual(sentTexts(loaded), ['詳しくは', '新しい'])
  })

  test('revert leaves text the page has changed since', () => {
    const p = h('p', {}, '一つ目')
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded, 1, 'ja')
    loaded.api.apply(1, 0, [{ text: 'First' }])
    p.childNodes[0].nodeValue = '三件の返信'
    loaded.api.revert()
    assert.equal(p.textContent, '三件の返信')
  })

  test('reverting stops watching the page', () => {
    const list = h('ul', {}, h('li', {}, '一つ目'))
    const loaded = load(page('https://example.jp/', list))
    goVisible(loaded, 1, 'ja')
    loaded.api.revert()
    assert.equal(loaded.mutations.at(-1).watching, false)
  })

  test('a page at the segment cap gains no more as it grows', () => {
    const list = h('ul', {}, ...Array.from({ length: 10000 }, (_, i) => h('li', {}, `項目${i}`)))
    const loaded = load(page('https://example.jp/', list))
    goVisible(loaded, 1, 'ja')
    const item = h('li', {}, '追加')
    list.appendChild(item)
    loaded.mutations.at(-1).fire(added(list, item))
    loaded.flush()
    loaded.observers.at(-1).show(item)
    loaded.flush()
    assert.equal(sentTexts(loaded).length, 10000)
  })

  test('a long page is translated past two thousand segments', () => {
    const paragraphs = Array.from({ length: 2500 }, (_, i) => h('p', {}, `段落${i}`))
    assert.equal(blocksOf(goVisible(load(page('https://example.jp/', ...paragraphs)))).length, 2500)
  })
})
