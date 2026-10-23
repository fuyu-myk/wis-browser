// chrome/peek.rs: PEEK_JS, which turns an Option-click on a link into a wisPeek preview request.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'
import { h, handlers, page } from '../support/s2-dom.mjs'

const SCRIPT = rawStringConst('chrome/peek.rs', 'PEEK_JS')

/** A page with one link to `href` reading `text`, running the Peek trigger. */
function loadPage(href, text = 'Read more', { channel = handlers('wisPeek') } = {}) {
  const inner = h('em', {}, text)
  const link = href == null ? h('a', {}, inner) : h('a', { href }, inner)
  const document = page('https://blog.test/post', h('p', {}, link))
  const window = channel ? { webkit: channel.webkit } : {}
  const run = () => new Function('window', 'document', SCRIPT)(window, document)
  run()
  const fire = (type, props = {}, target = link) => document.fire(type, { target, ...props })
  return {
    window,
    document,
    link,
    inner,
    run,
    sent: () => (channel ? channel.sent.map((m) => m.body) : []),
    press: (props, target) => fire('mousedown', props, target),
    click: (props, target) => fire('click', props, target),
  }
}

const OPTION = { altKey: true, clientX: 140, clientY: 62 }

test('Option+press on a link posts its href, text and click point, and cancels the press', () => {
  const p = loadPage('/next', 'Next  \n post')
  const press = p.press(OPTION)
  assert.deepEqual(p.sent(), [{ href: 'https://blog.test/next', text: 'Next post', x: 140, y: 62 }])
  assert.ok(press.defaultPrevented && press.propagationStopped)
})

test('the click that follows is cancelled too, without a second post', () => {
  const p = loadPage('/next')
  p.press(OPTION)
  const click = p.click(OPTION)
  assert.ok(click.defaultPrevented && click.propagationStopped)
  assert.equal(p.sent().length, 1)
})

test('an Option+click the page dispatched itself is left to the page', () => {
  const p = loadPage('/next')
  const press = p.press({ ...OPTION, isTrusted: false })
  const click = p.click({ ...OPTION, isTrusted: false })
  assert.ok(!press.defaultPrevented && !click.defaultPrevented)
  assert.deepEqual(p.sent(), [])
})

test('without Option, presses and clicks are left alone', () => {
  const p = loadPage('/next')
  for (const props of [{}, { metaKey: true }, { ctrlKey: true }, { shiftKey: true }]) {
    assert.equal(p.press(props).defaultPrevented, false)
    assert.equal(p.click(props).defaultPrevented, false)
  }
  assert.deepEqual(p.sent(), [])
})

test('Option with a button other than the primary one is left alone', () => {
  const p = loadPage('/next')
  assert.equal(p.press({ altKey: true, button: 1 }).defaultPrevented, false)
  assert.equal(p.click({ altKey: true, button: 2 }).defaultPrevented, false)
  assert.deepEqual(p.sent(), [])
})

test('javascript: links are never previewed, whatever their case', () => {
  for (const href of ['javascript:alert(1)', 'JavaScript:alert(1)']) {
    const p = loadPage(href)
    assert.equal(p.press(OPTION).defaultPrevented, false)
    assert.equal(p.click(OPTION).defaultPrevented, false)
    assert.deepEqual(p.sent(), [], href)
  }
})

test('other schemes are passed on for the native side to judge', () => {
  const p = loadPage('mailto:a@b.test')
  p.press(OPTION)
  assert.equal(p.sent()[0].href, 'mailto:a@b.test')
})

test('link text is capped at 200 characters', () => {
  const p = loadPage('/x', 'y'.repeat(300))
  p.press(OPTION)
  assert.equal(p.sent()[0].text.length, 200)
})

test('a press on an element or text inside the link finds the link', () => {
  const p = loadPage('/x')
  p.press(OPTION, p.inner)
  p.press(OPTION, p.inner.childNodes[0])
  assert.equal(p.sent().length, 2)
})

test('presses outside a link, on an anchor without href, or with no element target are ignored', () => {
  const bare = loadPage(null)
  assert.equal(bare.press(OPTION).defaultPrevented, false)
  const p = loadPage('/x')
  for (const target of [p.document.body, p.document, null]) p.press(OPTION, target)
  assert.deepEqual(p.sent(), [])
})

test('without the wisPeek handler the press fails quietly', () => {
  const p = loadPage('/x', 'x', { channel: null })
  assert.doesNotThrow(() => p.press(OPTION))
})

test('a handler that throws is contained', () => {
  const p = loadPage('/x')
  p.window.webkit.messageHandlers.wisPeek.postMessage = () => {
    throw new Error('gone')
  }
  assert.doesNotThrow(() => p.press(OPTION))
})

test('installing twice registers the listeners once', () => {
  const p = loadPage('/x')
  p.run()
  p.press(OPTION)
  assert.equal(p.sent().length, 1)
  assert.deepEqual(
    ['mousedown', 'click'].map((t) => p.document.listenerCount(t)),
    [1, 1],
  )
})
