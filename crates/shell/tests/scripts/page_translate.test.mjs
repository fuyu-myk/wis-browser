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

/** Runs the script over `doc` and returns its API, what it posted, and the observers. */
function load(doc) {
  const { webkit, sent } = handlers('wisTranslate')
  const window = eventTarget({ webkit })
  const { FakeObserver, observers } = observerClass()
  const timers = []
  const setTimeout = (fn) => timers.push(fn)
  new Function('window', 'document', 'NodeFilter', 'IntersectionObserver', 'setTimeout', SCRIPT)(
    window,
    doc,
    NodeFilter,
    FakeObserver,
    setTimeout,
  )
  const messages = () => sent.map(({ body }) => JSON.parse(body))
  const flush = () => {
    while (timers.length) timers.shift()()
  }
  return { api: window.__wisTranslate, window, messages, observers, flush }
}

/** Starts and runs translation generation `gen`, with every root on screen. */
function goVisible(loaded, gen = 1) {
  loaded.api.start(gen)
  loaded.api.go(gen)
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

  test('a segment that is one link is sent whole, untagged', () => {
    const doc = page('https://example.jp/', h('ul', {}, h('li', {}, h('a', { href: '/' }, 'ホーム'))))
    assert.deepEqual(blocksOf(goVisible(load(doc))), [{ id: 0, runs: [{ text: 'ホーム' }] }])
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
    assert.deepEqual(sent, [{ id: 1, runs: [{ text: '二つ目' }] }])
    assert.ok(!loaded.observers[0].targets.has(second), 'a sent block stops being watched')
  })

  test('a long page goes out in batches tagged with its generation', () => {
    const paragraphs = Array.from({ length: 50 }, (_, i) => h('p', {}, `段落${i}`))
    const messages = goVisible(load(page('https://example.jp/', ...paragraphs)), 7)
    assert.ok(messages.length > 1, 'more than one message')
    assert.ok(messages.every((m) => m.gen === 7 && m.blocks.length <= 24))
    assert.deepEqual(
      blocksOf(messages).map((b) => b.id),
      Array.from({ length: 50 }, (_, i) => i),
    )
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

  test('a translation that dropped a tag leaves the segment as it was', () => {
    const { loaded, p, before } = translatedParagraph()
    loaded.api.apply(1, 0, [{ text: 'Read the full text ' }, { text: 'here', tag: 0 }])
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
    assert.deepEqual(blocks, [{ id: 0, runs: [{ text: '一覧は' }, { text: 'ls', tag: 0 }, { text: 'で表示' }] }])
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

  test('a segment the page has since changed is skipped, not forced', () => {
    const { loaded, p } = translatedParagraph()
    p.childNodes[0].remove()
    p.insertBefore(new Text('置き換え'), p.childNodes[0])
    const changed = p.innerHTML
    loaded.api.apply(1, 0, [{ text: 'For more ' }, { text: 'text', tag: 1 }, { text: ' ' }, { text: 'here', tag: 0 }])
    assert.equal(p.innerHTML, changed)
  })
})
