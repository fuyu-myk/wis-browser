// reader.rs: MAIN_CONTENT_FINDER picks a page's article element; READER_BODY rebuilds the page around it.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'
import { h, page } from '../support/s2-dom.mjs'

const FINDER = rawStringConst('reader.rs', 'MAIN_CONTENT_FINDER')
const BODY = rawStringConst('reader.rs', 'READER_BODY')
const URL_NOW = 'https://site.test/story'

/** `n` characters of prose. */
const prose = (n) => 'word '.repeat(Math.ceil(n / 5)).slice(0, n)

function findMain(...children) {
  const doc = page(URL_NOW, ...children)
  return new Function('document', `${FINDER}\nreturn __wisFindMainContent();`)(doc)
}

describe('MAIN_CONTENT_FINDER', () => {
  test('an article with enough text is chosen over the page around it', () => {
    const article = h('article', {}, h('p', {}, prose(300)))
    const found = findMain(h('nav', {}, h('a', { href: '/' }, 'Home')), article, h('footer', {}, 'Copyright'))
    assert.equal(found, article)
  })

  test('every named container kind is recognised', () => {
    const kinds = [
      ['article', {}],
      ['main', {}],
      ['div', { role: 'main' }],
      ['div', { itemprop: 'articleBody' }],
      ['div', { class: 'post' }],
      ['div', { class: 'article' }],
      ['div', { class: 'entry-content' }],
      ['div', { class: 'post-content' }],
      ['div', { id: 'content' }],
      ['section', { class: 'content' }],
    ]
    for (const [tag, attrs] of kinds) {
      const container = h(tag, attrs, prose(250))
      assert.equal(findMain(h('div', {}, 'chrome'), container), container, JSON.stringify([tag, attrs]))
    }
  })

  test('of several named containers the one with the most text wins', () => {
    const smaller = h('article', {}, prose(250))
    const larger = h('div', { class: 'entry-content' }, prose(400))
    assert.equal(findMain(smaller, larger), larger)
  })

  test('a named container holding another wins when it has more text', () => {
    const article = h('article', {}, prose(250))
    const main = h('main', {}, article, h('div', {}, prose(100)))
    assert.equal(findMain(main), main)
  })

  test('named containers under 200 characters give way to the block with the most paragraph text', () => {
    const block = h('div', {}, h('p', {}, prose(100)), h('p', {}, prose(100)), h('p', {}, prose(100)))
    assert.equal(findMain(h('article', {}, prose(50)), block), block)
  })

  test('a block needs at least two paragraphs to count', () => {
    const section = h('section', {}, h('p', {}, prose(120)), h('p', {}, prose(120)))
    assert.equal(findMain(h('div', {}, h('p', {}, prose(500))), section), section)
  })

  test('the paragraph block must beat the short named container', () => {
    const article = h('article', {}, prose(150))
    assert.equal(findMain(article, h('div', {}, h('p', {}, prose(40)), h('p', {}, prose(40)))), article)
  })

  test('only paragraph text counts for a block', () => {
    const noisy = h('div', {}, h('p', {}, prose(30)), h('p', {}, prose(30)), h('span', {}, prose(1000)))
    const story = h('div', {}, h('p', {}, prose(100)), h('p', {}, prose(100)))
    assert.equal(findMain(noisy, story), story)
  })

  test('with no candidate at all the body is returned', () => {
    const found = findMain(h('span', {}, prose(50)))
    assert.equal(found.tagName, 'BODY')
  })

  test('a Wikipedia #mw-content-text holding over half the text is preferred to its container', () => {
    const body = h('div', { id: 'mw-content-text' }, h('p', {}, prose(400)))
    const content = h('div', { id: 'content' }, h('div', {}, prose(100)), body)
    assert.equal(findMain(content), body)
  })

  test('an inner articleBody holding half the text or less is not preferred', () => {
    const main = h('main', {}, h('div', { itemprop: 'articleBody' }, prose(200)), h('div', {}, prose(250)))
    assert.equal(findMain(main), main)
  })

  test('the tightening also applies after the paragraph fallback', () => {
    const inner = h('div', { id: 'mw-content-text' }, h('p', {}, prose(150)), h('p', {}, prose(150)))
    const outer = h('div', {}, inner, h('p', {}, prose(30)), h('p', {}, prose(30)))
    assert.equal(findMain(outer), inner)
  })

  test('whitespace is collapsed when measuring text', () => {
    const padded = h('article', {}, `${'   \n'.repeat(100)}short`)
    const block = h('div', {}, h('p', {}, prose(10)), h('p', {}, prose(10)))
    assert.equal(findMain(padded, block), block)
  })

  test('rendered text (innerText) is measured in preference to hidden text', () => {
    const article = h('article', {}, prose(300))
    Object.defineProperty(article, 'innerText', { value: 'short' })
    const block = h('div', {}, h('p', {}, prose(60)), h('p', {}, prose(60)))
    assert.equal(findMain(article, block), block)
  })

  test('the paragraph fallback picks the block holding the article, not a wrapper that adds its comments', () => {
    const story = h('div', { class: 'story' }, h('p', {}, prose(100)), h('p', {}, prose(100)), h('p', {}, prose(100)))
    const comments = h('div', { class: 'comments' }, h('p', {}, prose(120)), h('p', {}, prose(120)))
    assert.equal(findMain(h('div', { class: 'wrap' }, story, comments)), story)
  })

  test('a comment thread or sidebar is never the article, however long', () => {
    const story = h('div', {}, h('p', {}, prose(100)), h('p', {}, prose(100)))
    const comments = h('section', { id: 'comments' }, h('p', {}, prose(400)), h('p', {}, prose(400)))
    const sidebar = h('div', { class: 'sidebar' }, h('p', {}, prose(400)), h('p', {}, prose(400)))
    assert.equal(findMain(story, comments, sidebar), story)
  })

  test('an article split into sections keeps the block holding them all', () => {
    const wrap = h(
      'div',
      {},
      h('section', {}, h('p', {}, prose(100)), h('p', {}, prose(100))),
      h('section', {}, h('p', {}, prose(100)), h('p', {}, prose(100))),
    )
    assert.equal(findMain(wrap), wrap)
  })

  test('with no body there is nothing to find', () => {
    const doc = page(URL_NOW)
    Object.defineProperty(doc, 'body', { value: null })
    assert.equal(new Function('document', `${FINDER}\nreturn __wisFindMainContent();`)(doc), null)
  })
})

/**
 * Run the reader transform over a page. `bodyBg` and `rootBg` are what
 * getComputedStyle reports for body and html.
 */
function readerize(children, { title = 'A story', bodyBg = 'rgb(255, 255, 255)', rootBg = 'rgba(0, 0, 0, 0)', style } = {}) {
  const doc = page(URL_NOW, ...children)
  doc.title = title
  const getComputedStyle =
    style ?? ((el) => ({ backgroundColor: el === doc.body ? bodyBg : el === doc.documentElement ? rootBg : '' }))
  new Function('document', 'getComputedStyle', `${FINDER}\n${BODY}`)(doc, getComputedStyle)
  return { doc, html: doc.documentElement.html }
}

const story = (...extra) => h('article', {}, h('p', {}, prose(300)), ...extra)

describe('READER_BODY', () => {
  test('the article is rebuilt as a kbreader page headed by the document title', () => {
    const { doc, html } = readerize([h('nav', {}, 'Menu'), story(h('h2', {}, 'Part two'))], { title: '  A story  ' })
    assert.match(html, /^<head><meta charset="utf-8"><meta name="viewport"[^>]*><style>[^<]*<\/style><\/head><body>/)
    assert.ok(html.includes(`<main class=kbreader><h1 class=kbreader-title>A story</h1><p>${prose(300)}</p><h2>Part two</h2></main>`))
    assert.ok(!html.includes('Menu'), 'only the article is kept')
    assert.equal(doc.title, 'A story')
  })

  test('page furniture inside the article is dropped', () => {
    const junk = [
      h('script', {}, 'JUNK'),
      h('style', {}, 'JUNK'),
      h('link', { rel: 'stylesheet' }),
      h('noscript', {}, 'JUNK'),
      h('iframe', {}, 'JUNK'),
      h('form', {}, 'JUNK'),
      h('nav', {}, 'JUNK'),
      h('aside', {}, 'JUNK'),
      h('header', {}, 'JUNK'),
      h('footer', {}, 'JUNK'),
      h('button', {}, 'JUNK'),
      h('svg', {}, 'JUNK'),
      h('div', { role: 'navigation' }, 'JUNK'),
      ...[
        'mw-editsection',
        'navbox',
        'vector-page-toolbar',
        'mw-indicators',
        'mw-jump-link',
        'reflist',
        'mw-references-wrap',
        'infobox',
        'sidebar',
        'metadata',
        'ambox',
        'noprint',
        'mw-empty-elt',
        'shortdescription',
        'hatnote',
      ].map((cls) => h('div', { class: cls }, 'JUNK')),
    ]
    const { html } = readerize([story(...junk)])
    assert.ok(!html.includes('JUNK'))
    assert.ok(!html.includes('<link'))
    assert.ok(html.includes(prose(300)))
  })

  test('the live page is not modified while the article is cleaned', () => {
    const script = h('script', {}, 'keep me')
    const article = story(script)
    readerize([article])
    assert.equal(script.parentNode, article)
  })

  test('only href, src and alt attributes survive', () => {
    const link = h('a', { href: '/next', class: 'btn', onclick: 'track()' }, 'next')
    const img = h('img', { src: '/i.png', alt: 'pic', width: '10', style: 'float:left' })
    const { html } = readerize([story(h('p', { id: 'p2', 'data-x': '1' }, link, img))])
    assert.ok(html.includes('<p><a href="/next">next</a><img src="/i.png" alt="pic"></p>'))
    for (const gone of ['onclick', 'btn', 'width="10"', 'float', 'data-x', 'p2']) assert.ok(!html.includes(gone), gone)
  })

  test('the title is escaped so it cannot add markup', () => {
    const { html } = readerize([story()], { title: '<img src=x onerror=alert(1)>' })
    assert.ok(html.includes('<h1 class=kbreader-title>&lt;img src=x onerror=alert(1)></h1>'))
    assert.ok(!html.includes('<img src=x'))
  })

  test('without a title there is no heading', () => {
    for (const title of ['', '   ']) {
      const { html } = readerize([story()], { title })
      assert.ok(!html.includes('<h1'), JSON.stringify(title))
    }
  })

  test('a dark page gets the dark palette and a light one the light palette', () => {
    const dark = readerize([story()], { bodyBg: 'rgb(20, 20, 24)' }).html
    assert.ok(dark.includes('background:#16161a;color:#e9e9ee'))
    assert.ok(dark.includes('main.kbreader a{color:#9aa4ff}'))
    const light = readerize([story()], { bodyBg: 'rgb(250, 250, 250)' }).html
    assert.ok(light.includes('background:#fbfbfd;color:#1d1d22'))
    assert.ok(light.includes('main.kbreader a{color:#6b77f8}'))
  })

  test('a transparent body defers to the root background, and with neither set the page counts as white', () => {
    const fromRoot = readerize([story()], { bodyBg: 'rgba(0, 0, 0, 0)', rootBg: 'rgb(0, 0, 0)' }).html
    assert.ok(fromRoot.includes('background:#16161a'))
    const unset = readerize([story()], { bodyBg: 'transparent', rootBg: 'rgba(0, 0, 0, 0)' }).html
    assert.ok(unset.includes('background:#fbfbfd'))
  })

  test('a failure anywhere leaves the page as it was', () => {
    const article = story()
    const { doc, html } = readerize([article], {
      title: 'Kept',
      style: () => {
        throw new Error('no style')
      },
    })
    assert.equal(html, undefined)
    assert.equal(doc.title, 'Kept')
    assert.equal(article.parentNode, doc.body)
  })
})
