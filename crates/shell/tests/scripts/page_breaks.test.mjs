// The page script's line breaks inside a sentence. Both models end a
// paragraph at a line break, so a `<br>` the page hides, and on a Japanese
// page one that cuts a phrase, joins the lines around it, and a line break
// the page folds into a space is sent as one.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { Text, h, page } from '../support/s2-dom.mjs'
import { blocksOf, goVisible, load } from '../support/translatePage.mjs'

const textsOf = (messages) => blocksOf(messages).map((b) => b.runs.map((r) => r.text).join(''))

function hiddenBreak() {
  const br = h('br', { class: 'sp' })
  br.style.display = 'none'
  return br
}

const TRADE_IN = 'If you trade it in, the new product will be discounted.'

describe('line breaks inside a sentence', () => {
  test('a line break the page hides does not end a sentence', () => {
    const ja = h('p', {}, 'これはただの世代交代では', hiddenBreak(), 'なく、革新です。')
    const en = h('p', {}, 'Not just a new generation,', hiddenBreak(), ' but a leap.')
    assert.deepEqual(textsOf(goVisible(load(page('https://example.jp/', ja)), 1, 'ja')), [
      'これはただの世代交代ではなく、革新です。',
    ])
    assert.deepEqual(textsOf(goVisible(load(page('https://example.com/', en)), 1, 'en')), [
      'Not just a new generation, but a leap.',
    ])
  })

  test('on a Japanese page a break that cuts a phrase joins its lines', () => {
    const heading = h('h2', {}, '下取りに出すと、', h('br'), '新しい製品が割引になります。')
    const copy = h('p', {}, '究極の性能を', h('br'), '誇るiPhone。')
    const texts = textsOf(goVisible(load(page('https://example.jp/', heading, copy)), 1, 'ja'))
    assert.deepEqual(texts, ['下取りに出すと、新しい製品が割引になります。', '究極の性能を誇るiPhone。'])
  })

  test('a break after a sentence or a label, or on a page in another language, still ends one', () => {
    const sentence = h('p', {}, '革新です。', h('br'), 'さあ、新しい世界に')
    const label = h('p', {}, '渋谷区', h('br'), '渋谷は世界を変える。')
    const links = h('p', {}, h('a', { href: '/news' }, 'お知らせはこちら'), h('br'), h('a', { href: '/jobs' }, '採用情報'))
    const polite = h('p', {}, 'お問い合わせください', h('br'), '受付時間は平日のみ')
    const ja = textsOf(goVisible(load(page('https://example.jp/', sentence, label, links, polite)), 1, 'ja'))
    assert.deepEqual(ja, [
      '革新です。',
      'さあ、新しい世界に',
      '渋谷区',
      '渋谷は世界を変える。',
      'お知らせはこちら',
      '採用情報',
      'お問い合わせください',
      '受付時間は平日のみ',
    ])
    const listed = h('p', {}, '苹果、', h('br'), '香蕉')
    assert.deepEqual(textsOf(goVisible(load(page('https://example.cn/', listed)), 1, 'zh')), ['苹果、', '香蕉'])
  })

  test('a joined break is hidden while translated and shown again on revert', () => {
    const br = h('br')
    const heading = h('h2', {}, '下取りに出すと、', br, '新しい製品が割引になります。')
    const loaded = load(page('https://example.jp/', heading))
    const before = heading.innerHTML
    goVisible(loaded, 1, 'ja')
    loaded.api.apply(1, 0, [{ text: TRADE_IN }])
    assert.equal(heading.textContent, TRADE_IN)
    assert.equal(br.style.display, 'none')
    loaded.api.revert()
    assert.equal(br.style.display, undefined)
    assert.equal(heading.innerHTML, before)
  })

  test('a shown break with a style of its own still ends the sentence', () => {
    const p = h('p', {}, '下取りに出すと、', h('br', { style: 'clear: both' }), '新しい製品が割引になります。')
    const texts = textsOf(goVisible(load(page('https://example.jp/', p)), 1, 'ja'))
    assert.deepEqual(texts, ['下取りに出すと、', '新しい製品が割引になります。'])
  })

  test('a sentence joined across a break stays translated when the page changes', () => {
    const heading = h('h2', {}, '下取りに出すと、', h('br'), '新しい製品が割引になります。')
    const other = h('p', {}, '別の文です。')
    const loaded = load(page('https://example.jp/', heading, other))
    goVisible(loaded, 1, 'ja')
    loaded.api.apply(1, 0, [{ text: TRADE_IN }])
    const first = heading.childNodes[0]
    const writes = []
    let value = first.nodeValue
    Object.defineProperty(first, 'nodeValue', {
      get: () => value,
      set: (text) => {
        writes.push(text)
        value = text
      },
    })
    const added = new Text('追加')
    other.appendChild(added)
    loaded.mutations.at(-1).fire({ type: 'childList', target: other, addedNodes: [added], removedNodes: [] })
    loaded.flush()
    assert.deepEqual(writes, [])
    assert.equal(heading.textContent, TRADE_IN)
  })

  test('a line break the page folds into a space is sent as one', () => {
    const joined = h('p', {}, 'サービスを通じ、', h('br'), '\n      人と社会に貢献します。')
    const kept = h('p', {}, '一行目\n二行目')
    kept.style.whiteSpace = 'pre-line'
    const texts = textsOf(goVisible(load(page('https://example.jp/', joined, kept)), 1, 'ja'))
    assert.deepEqual(texts, ['サービスを通じ、 人と社会に貢献します。', '一行目\n二行目'])
  })

  test('a sentence sent with its line breaks folded still takes its translation', () => {
    const p = h('p', {}, '前半の文、\n  後半の文です。')
    const loaded = load(page('https://example.jp/', p))
    goVisible(loaded, 1, 'ja')
    loaded.api.apply(1, 0, [{ text: 'The first half, the second half.' }])
    assert.equal(p.textContent, 'The first half, the second half.')
  })
})
