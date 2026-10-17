// win_dispatch.rs: SHIM_SCRIPT and AI_FETCH_SHIM, which rebuild webkit.messageHandlers over WebView2's single channel,
// and private_script, which keeps a script's acting channels behind a sender only it holds.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { formatTemplate, rawStringConst } from '../support/rustSource.mjs'
import { fnBody, shellSource } from '../support/s2-rust.mjs'

const SHIM = rawStringConst('win_dispatch.rs', 'SHIM_SCRIPT')
const AI_FETCH_SHIM = rawStringConst('win_dispatch.rs', 'AI_FETCH_SHIM')

// Every handler name the native receivers route: the tab dispatcher's match
// arms, plus the panel receiver's two open-in-tab channels.
const ROUTED = [
  ...[...fnBody('win_dispatch.rs', 'dispatch').matchAll(/^\s*"(wis\w+)"(?:\s+if\s+\w+)?\s*=>/gm)].map((m) => m[1]),
  ...[...shellSource('link_open.rs').matchAll(/const HANDLER_\w+: &str = "(wis\w+)";/g)].map((m) => m[1]),
]
// The channels that post only through private_script's sender.
const ACTING = [
  ...shellSource('win_dispatch.rs')
    .match(/const ACTING: &\[&str\] = &\[([^\]]*)\]/)[1]
    .matchAll(/"(wis\w+)"/g),
].map((m) => m[1])

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

  test('every routed page-world channel has a shim handler, and no acting channel does', () => {
    const p = loadPage(SHIM)
    assert.ok(ROUTED.length >= 13, `parsed ${ROUTED.length} routed names`)
    assert.ok(ACTING.length >= 8, `parsed ${ACTING.length} acting names`)
    for (const name of ROUTED) assert.equal(Boolean(p.handler(name)), !ACTING.includes(name), name)
  })

  test('string bodies stay strings inside the envelope', () => {
    const p = loadPage(SHIM)
    p.handler('wisZap').postMessage('div.ad')
    assert.deepEqual(p.envelopes(), [{ handler: 'wisZap', body: 'div.ad' }])
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
    const webkit = { messageHandlers: { wisCosmetic: own }, other: 1 }
    const p = loadPage(SHIM, { webkit })
    assert.equal(p.window.webkit, webkit)
    assert.equal(p.handler('wisCosmetic'), own)
    assert.ok(p.handler('wisZap'))
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
    assert.doesNotThrow(() => p.handler('wisCosmetic').postMessage(cyclic))
    assert.deepEqual(p.sent, [])
  })

  test('a channel that throws is contained', () => {
    const p = loadPage(SHIM)
    p.window.chrome.webview.postMessage = () => {
      throw new Error('closed')
    }
    assert.doesNotThrow(() => p.handler('wisCosmetic').postMessage({}))
  })

  test('the channel is looked up at post time', () => {
    const p = loadPage(SHIM)
    const later = []
    p.window.chrome.webview = { postMessage: (m) => later.push(m) }
    p.handler('wisZap').postMessage({ selector: 'x' })
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

const NAMES = ['wisPeek', 'wisKeybind']

/** `source` wrapped as Rust wraps it, with `frozen` fixed behind the token. */
function wrap(source, frozen = []) {
  return formatTemplate('win_dispatch.rs', 'private_script', {
    token: 't1',
    channels: NAMES.map((name) => `${name}: channel('${name}')`).join(', '),
    source: source.replace(/window\.webkit/g, 'webkit'),
    freeze: frozen.map((name) => `  freeze('${name}');\n`).join(''),
  })
}

/** A WebView2 page running `script`; `json` stands in for the page's JSON. */
function loadWrapped(script, { webview = true } = {}) {
  const sent = []
  const json = { stringify: JSON.stringify }
  const window = {}
  if (webview) window.chrome = { webview: { postMessage: (text) => sent.push(text) } }
  new Function('window', 'JSON', script)(window, json)
  return { window, json, sent, posts: () => sent.map((text) => JSON.parse(text)) }
}

const POSTER = `window.__wisFire = function (body) {
  window.webkit.messageHandlers.wisPeek.postMessage(body);
};`

describe('private_script', () => {
  test("a post carries its channel, the webview's token and the body", () => {
    const p = loadWrapped(wrap(POSTER))
    p.window.__wisFire({ href: 'https://a.test/', x: 1 })
    p.window.__wisFire(undefined)
    assert.deepEqual(p.posts(), [
      { handler: 'wisPeek', token: 't1', body: { href: 'https://a.test/', x: 1 } },
      { handler: 'wisPeek', token: 't1', body: null },
    ])
  })

  test('the page gets no handler of its own to post through', () => {
    const p = loadWrapped(wrap(POSTER))
    assert.equal(p.window.webkit, undefined)
  })

  test('replacing postMessage or JSON.stringify later never shows the page the token', () => {
    const p = loadWrapped(wrap(POSTER))
    const seen = []
    p.window.chrome.webview.postMessage = (text) => seen.push(text)
    p.json.stringify = (value) => {
      seen.push(value)
      return '0'
    }
    p.window.__wisFire({ x: 1 })
    assert.deepEqual(seen, [])
    assert.equal(p.posts()[0].token, 't1')
  })

  test('a frozen function answers only a call that brings the token, and stays in place', () => {
    const p = loadWrapped(
      wrap('window.calls = []; window.__wisKeybindUpdate = function (map) { window.calls.push(map); };', [
        '__wisKeybindUpdate',
      ]),
    )
    p.window.__wisKeybindUpdate({ new_tab: 'Ctrl+A' })
    p.window.__wisKeybindUpdate({ new_tab: 'Ctrl+A' }, 'guess')
    assert.throws(() => {
      p.window.__wisKeybindUpdate = () => {}
    }, TypeError)
    p.window.__wisKeybindUpdate({ new_tab: 'Ctrl+T' }, 't1')
    assert.deepEqual(p.window.calls, [{ new_tab: 'Ctrl+T' }])
  })

  test('without the WebView2 channel the script does not run', () => {
    const p = loadWrapped(wrap(POSTER), { webview: false })
    assert.equal(p.window.__wisFire, undefined)
  })
})

test('every channel the wrapped scripts post to is one the private sender carries', () => {
  const scripts = [
    ['link_open.rs', 'OPEN_IN_TAB_JS'],
    ['link_open.rs', 'PANEL_CROSS_SITE_JS'],
    ['peek.rs', 'PEEK_JS'],
    ['page_menu.rs', 'PAGE_MENU_JS'],
    ['signup.rs', 'SIGNUP_JS'],
    ['capture.rs', 'CAPTURE_JS'],
    ['navguard.rs', 'NAVGUARD_JS'],
    ['keybind.rs', 'KEYBIND_JS'],
    ['video_popout/script.rs', 'POPOUT_JS'],
  ]
  for (const [file, name] of scripts) {
    const posted = [...rawStringConst(file, name).matchAll(/(?:messageHandlers\.|\b)(wis[A-Z]\w*)\.postMessage|messageHandlers\.(wis\w+)/g)].map(
      (m) => m[1] || m[2],
    )
    assert.ok(posted.length, `${name} posts somewhere`)
    for (const channel of posted) assert.ok(ACTING.includes(channel), `${name} posts to ${channel}`)
  }
})

describe('the canvas shim (Windows)', () => {
  const SHIM_CANVAS = formatTemplate('canvas.rs', 'shim_script', { token: 't1' })
  /** A WebView2 window after the canvas shim runs; `top` false makes it a card's frame. */
  const loadCanvas = ({ top = true, webview = true } = {}) => {
    const sent = []
    const window = webview ? { chrome: { webview: { postMessage: (text) => sent.push(text) } } } : {}
    window.top = top ? window : {}
    new Function('window', SHIM_CANVAS)(window)
    return { window, sent }
  }

  test('the canvas page posts each message sealed with its tab token', () => {
    const p = loadCanvas()
    p.window.webkit.messageHandlers.wisCanvas.postMessage('{"id":1,"op":"list"}')
    assert.deepEqual(p.sent.map((t) => JSON.parse(t)), [{ handler: 'wisCanvas', token: 't1', body: '{"id":1,"op":"list"}' }])
  })

  test('a card frame gets no channel and never holds the token', () => {
    const p = loadCanvas({ top: false })
    assert.equal(p.window.webkit, undefined)
    assert.deepEqual(p.sent, [])
  })

  test('outside WebView2 the shim does nothing', () => {
    assert.equal(loadCanvas({ webview: false }).window.webkit, undefined)
  })
})
