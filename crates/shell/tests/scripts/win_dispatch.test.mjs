// win_dispatch.rs: SHIM_SCRIPT and AI_FETCH_SHIM, which rebuild webkit.messageHandlers over WebView2's single channel.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'
import { fnBody, shellSource } from '../support/s2-rust.mjs'

const SHIM = rawStringConst('win_dispatch.rs', 'SHIM_SCRIPT')
const AI_FETCH_SHIM = rawStringConst('win_dispatch.rs', 'AI_FETCH_SHIM')

// Every handler name the native receivers route: the tab dispatcher's match
// arms, plus the panel receiver's two open-in-tab channels.
const ROUTED = [
  ...[...fnBody('win_dispatch.rs', 'dispatch').matchAll(/^\s*"(wis\w+)"(?:\s+if\s+\w+)?\s*=>/gm)].map((m) => m[1]),
  ...[...shellSource('link_open.rs').matchAll(/const HANDLER_\w+: &str = "(wis\w+)";/g)].map((m) => m[1]),
]

/** A WebView2-like window (or a plain one with `webview: false`) after the shim runs. */
function loadPage(script, { webview = true, webkit } = {}) {
  const sent = []
  const window = {}
  if (webview) window.chrome = { webview: { postMessage: (message) => sent.push(message) } }
  if (webkit) window.webkit = webkit
  const run = () => new Function('window', script)(window)
  run()
  return {
    window,
    run,
    sent,
    envelopes: () => sent.map((m) => JSON.parse(m)),
    handler: (name) => window.webkit.messageHandlers[name],
  }
}

describe('SHIM_SCRIPT', () => {
  test('each named handler forwards its body as a { handler, body } JSON string', () => {
    const p = loadPage(SHIM)
    const names = Object.keys(p.window.webkit.messageHandlers)
    for (const name of names) p.handler(name).postMessage({ href: 'https://a.test/', n: 1 })
    assert.ok(p.sent.every((m) => typeof m === 'string'), 'TryGetWebMessageAsString needs a string')
    assert.deepEqual(
      p.envelopes(),
      names.map((handler) => ({ handler, body: { href: 'https://a.test/', n: 1 } })),
    )
  })

  test('every channel the native receivers route has a shim handler', () => {
    const p = loadPage(SHIM)
    assert.ok(ROUTED.length >= 13, `parsed ${ROUTED.length} routed names`)
    for (const name of ROUTED) assert.ok(p.handler(name), name)
  })

  test('string bodies stay strings inside the envelope', () => {
    const p = loadPage(SHIM)
    p.handler('wisOpenInTab').postMessage('https://a.test/')
    assert.deepEqual(p.envelopes(), [{ handler: 'wisOpenInTab', body: 'https://a.test/' }])
  })

  test('an undefined body is sent as null', () => {
    const p = loadPage(SHIM)
    p.handler('wisZap').postMessage()
    assert.equal(p.sent[0], '{"handler":"wisZap","body":null}')
  })

  test('tab pages get no AI fetch channel', () => {
    const p = loadPage(SHIM)
    assert.equal(p.handler('wisAiFetch'), undefined)
  })

  test('handlers and a webkit object the page already has are kept', () => {
    const own = { postMessage: () => 'own' }
    const webkit = { messageHandlers: { wisPeek: own }, other: 1 }
    const p = loadPage(SHIM, { webkit })
    assert.equal(p.window.webkit, webkit)
    assert.equal(p.handler('wisPeek'), own)
    assert.ok(p.handler('wisNavGuard'))
  })

  test('outside WebView2 the shim does nothing', () => {
    const p = loadPage(SHIM, { webview: false })
    assert.equal(p.window.webkit, undefined)
    const noWebview = { chrome: {} }
    new Function('window', SHIM)(noWebview)
    assert.equal(noWebview.webkit, undefined)
  })

  test('a body that cannot be serialised is dropped without throwing', () => {
    const p = loadPage(SHIM)
    const cyclic = {}
    cyclic.self = cyclic
    assert.doesNotThrow(() => p.handler('wisPeek').postMessage(cyclic))
    assert.deepEqual(p.sent, [])
  })

  test('a channel that throws is contained', () => {
    const p = loadPage(SHIM)
    p.window.chrome.webview.postMessage = () => {
      throw new Error('closed')
    }
    assert.doesNotThrow(() => p.handler('wisPeek').postMessage({}))
  })

  test('the channel is looked up at post time', () => {
    const p = loadPage(SHIM)
    const later = []
    p.window.chrome.webview = { postMessage: (m) => later.push(m) }
    p.handler('wisKeybind').postMessage({ action: 'x' })
    assert.deepEqual(p.sent, [])
    assert.equal(later.length, 1)
  })

  test('running the shim twice keeps the first handlers', () => {
    const p = loadPage(SHIM)
    const first = p.handler('wisCosmetic')
    p.run()
    assert.equal(p.handler('wisCosmetic'), first)
  })
})

describe('AI_FETCH_SHIM', () => {
  test('only the wisAiFetch channel exists, forwarding { handler, body } as a JSON string', () => {
    const p = loadPage(AI_FETCH_SHIM)
    assert.deepEqual(Object.keys(p.window.webkit.messageHandlers), ['wisAiFetch'])
    p.handler('wisAiFetch').postMessage({ text: 'page', title: 'T' })
    assert.deepEqual(p.sent, ['{"handler":"wisAiFetch","body":{"text":"page","title":"T"}}'])
  })

  test('an undefined body is sent as null', () => {
    const p = loadPage(AI_FETCH_SHIM)
    p.handler('wisAiFetch').postMessage()
    assert.deepEqual(p.envelopes(), [{ handler: 'wisAiFetch', body: null }])
  })

  test('outside WebView2 the shim does nothing', () => {
    const p = loadPage(AI_FETCH_SHIM, { webview: false })
    assert.equal(p.window.webkit, undefined)
  })

  test('an existing wisAiFetch handler and webkit object are kept', () => {
    const own = { postMessage: () => {} }
    const webkit = { messageHandlers: { wisAiFetch: own } }
    const p = loadPage(AI_FETCH_SHIM, { webkit })
    assert.equal(p.window.webkit, webkit)
    assert.equal(p.handler('wisAiFetch'), own)
  })

  test('unserialisable bodies and a throwing channel are contained', () => {
    const p = loadPage(AI_FETCH_SHIM)
    const cyclic = {}
    cyclic.self = cyclic
    assert.doesNotThrow(() => p.handler('wisAiFetch').postMessage(cyclic))
    p.window.chrome.webview.postMessage = () => {
      throw new Error('closed')
    }
    assert.doesNotThrow(() => p.handler('wisAiFetch').postMessage({ text: 'x' }))
  })
})
