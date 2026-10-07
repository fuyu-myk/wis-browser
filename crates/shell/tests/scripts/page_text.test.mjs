// `page_text.rs` extract_script: posts the page's main readable text and title to `wisPageText` for the assistant.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'

// The extraction is a raw-string argument to `format!`, not a template, so it
// is read out of `fn extract_script` directly.
const SOURCE = readFileSync(new URL('../../src-tauri/src/page_text.rs', import.meta.url), 'utf8')
const FN_AT = SOURCE.indexOf('fn extract_script')
const BODY_AT = SOURCE.indexOf('r#"', FN_AT) + 3
const BODY = SOURCE.slice(BODY_AT, SOURCE.indexOf('"#', BODY_AT))
const FINDER = rawStringConst('reader.rs', 'MAIN_CONTENT_FINDER')
// As `format!("{}\n{}", MAIN_CONTENT_FINDER, body)` assembles it.
const SCRIPT = `${FINDER}\n${BODY}`

const CAP = 150000

/** An element as the finder and the extraction read it. */
function el(innerText, textContent = innerText) {
  return { innerText, textContent, querySelector: () => null, querySelectorAll: () => [] }
}

/** A window posting through `channel`: `webkit`, `window` or `none`. */
function windowWith(channel, sent) {
  const window = {}
  const handler = { postMessage: (body) => sent.push(body) }
  if (channel === 'webkit') window.webkit = { messageHandlers: { wisPageText: handler } }
  if (channel === 'window') window.wisPageText = handler
  return window
}

/** Runs the extraction alone, with `best` as what the finder returns. */
function extract(best, { title = 'A page', channel = 'webkit', finder } = {}) {
  const sent = []
  const window = windowWith(channel, sent)
  const document = { title }
  new Function('window', 'document', '__wisFindMainContent', BODY)(window, document, finder ?? (() => best))
  return { sent, window }
}

/** Runs the script as shipped, shared finder included, over a page. */
function extractPage({ named = [], blocks = [], body = el(''), title = 'A page' }) {
  const sent = []
  const document = {
    title,
    body,
    querySelectorAll: (selector) => {
      if (selector.startsWith('article, main')) return named
      if (selector === 'div, section') return blocks
      throw new Error(`unexpected selector ${selector}`)
    },
  }
  new Function('window', 'document', SCRIPT)(windowWith('webkit', sent), document)
  return sent
}

describe('the script as shipped', () => {
  test('is the shared main-content finder followed by the extraction', () => {
    const call = SOURCE.slice(FN_AT, BODY_AT)
    assert.match(call, /format!\(\s*"\{\}\\n\{\}",\s*crate::reader::MAIN_CONTENT_FINDER,\s*r#"$/)
  })

  test("posts the text of the finder's main content, not the whole page", () => {
    const article = el(`Lead.   ${'word '.repeat(60)}`)
    const sent = extractPage({ named: [article], body: el('Menu Lead. Footer') })
    assert.equal(sent.length, 1)
    assert.equal(sent[0].text, `Lead. ${'word '.repeat(60).trim()}`)
  })

  test("falls back to the body's text on a page with no main content", () => {
    const sent = extractPage({ body: el('Just a body') })
    assert.deepEqual(sent, [{ text: 'Just a body', title: 'A page' }])
  })

  test.todo(
    'a document without a body answers with empty text (bug: __wisFindMainContent dereferences a null document.body, the throw is swallowed and nothing is posted, so the assistant waits out its 6 s timeout; the extraction guard `best &&` never applies)',
  )
})

describe('the text', () => {
  test('posts exactly the text and title fields the native handler reads', () => {
    assert.deepEqual(extract(el('Hello')).sent, [{ text: 'Hello', title: 'A page' }])
  })

  test('innerText is preferred, so hidden text is left out', () => {
    assert.equal(extract(el('visible', 'visible hidden')).sent[0].text, 'visible')
  })

  test('textContent stands in when there is no innerText', () => {
    assert.equal(extract(el(undefined, 'from textContent')).sent[0].text, 'from textContent')
    assert.equal(extract(el('', 'from textContent')).sent[0].text, 'from textContent')
  })

  test('an element with neither, or no element at all, posts empty text', () => {
    assert.equal(extract(el(undefined, null)).sent[0].text, '')
    assert.equal(extract(null).sent[0].text, '')
  })

  test('whitespace runs collapse to one space and the ends are trimmed', () => {
    assert.equal(extract(el('\n  One\t\ttwo\n\nthree  ')).sent[0].text, 'One two three')
  })

  test('text is capped at 150,000 characters', () => {
    assert.equal(extract(el('x'.repeat(CAP))).sent[0].text.length, CAP)
    assert.equal(extract(el('x'.repeat(CAP + 1))).sent[0].text.length, CAP)
  })

  test('the title is trimmed, and a missing one is empty', () => {
    assert.equal(extract(el('t'), { title: '  Title  ' }).sent[0].title, 'Title')
    assert.equal(extract(el('t'), { title: null }).sent[0].title, '')
  })
})

describe('the channel', () => {
  test('without the webkit handler it posts the same object to window.wisPageText', () => {
    assert.deepEqual(extract(el('Hello'), { channel: 'window' }).sent, [{ text: 'Hello', title: 'A page' }])
  })

  test('with no channel at all nothing is posted and nothing throws', () => {
    assert.doesNotThrow(() => extract(el('Hello'), { channel: 'none' }))
  })

  test('a finder or handler that throws is swallowed, and nothing is posted', () => {
    const throwing = () => {
      throw new Error('boom')
    }
    assert.deepEqual(extract(null, { finder: throwing }).sent, [])

    const window = { webkit: { messageHandlers: { wisPageText: { postMessage: throwing } } } }
    assert.doesNotThrow(() =>
      new Function('window', 'document', '__wisFindMainContent', BODY)(window, { title: '' }, () => el('x')),
    )
  })
})
