// permissions.rs: GATE_JS, which holds getUserMedia until wisMediaPermission returns the user's decision.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'
import { handlers } from '../support/s2-dom.mjs'

const SCRIPT = rawStringConst('permissions.rs', 'GATE_JS')

/**
 * A page with a MediaDevices whose real getUserMedia records its calls.
 * `media: false` drops MediaDevices; `gum` replaces the real getUserMedia.
 */
function loadPage({ media = true, gum, channel = handlers('wisMediaPermission') } = {}) {
  const calls = []
  const stream = { id: 'stream' }
  class MediaDevices {}
  MediaDevices.prototype.getUserMedia =
    gum !== undefined
      ? gum
      : function (constraints) {
          calls.push({ self: this, constraints })
          return Promise.resolve(stream)
        }
  const window = media ? { MediaDevices } : {}
  if (channel) window.webkit = channel.webkit
  const location = { origin: 'https://meet.test' }
  const run = () => new Function('window', 'location', SCRIPT)(window, location)
  run()
  const devices = new MediaDevices()
  return {
    window,
    calls,
    stream,
    devices,
    run,
    proto: MediaDevices.prototype,
    sent: () => (channel ? channel.sent.map((m) => m.body) : []),
    ask: (constraints) => devices.getUserMedia(constraints),
    // The exact call the native side evaluates to answer a request.
    resolve: (id, granted) =>
      new Function('window', `window.__wisMediaResolve && window.__wisMediaResolve("${id}", ${granted});`)(window),
  }
}

const settled = (promise) =>
  Promise.race([promise.then(() => 'settled', () => 'settled'), new Promise((r) => setImmediate(() => r('pending')))])

test('getUserMedia asks wisMediaPermission with a numeric id, the page origin and the kind, and waits', async () => {
  const p = loadPage()
  const pending = p.ask({ video: true, audio: true })
  assert.deepEqual(p.sent(), [{ id: '1', origin: 'https://meet.test', kind: 'camera+microphone' }])
  assert.equal(await settled(pending), 'pending')
  assert.equal(p.calls.length, 0, 'the device is untouched until the user decides')
})

test('a grant calls the real getUserMedia on the same object with the same constraints', async () => {
  const p = loadPage()
  const constraints = { video: { width: 640 } }
  const pending = p.ask(constraints)
  p.resolve('1', true)
  assert.equal(await pending, p.stream)
  assert.equal(p.calls.length, 1)
  assert.equal(p.calls[0].self, p.devices)
  assert.equal(p.calls[0].constraints, constraints)
})

test('a denial rejects with NotAllowedError without touching the device', async () => {
  const p = loadPage()
  const pending = p.ask({ audio: true })
  p.resolve('1', false)
  await assert.rejects(pending, (err) => err instanceof DOMException && err.name === 'NotAllowedError')
  assert.equal(p.calls.length, 0)
})

test('the kind names camera, microphone or both, and falls back to camera', () => {
  const p = loadPage()
  for (const constraints of [{ video: true }, { audio: true }, { video: true, audio: true }, {}, undefined]) {
    p.ask(constraints)
  }
  assert.deepEqual(
    p.sent().map((m) => m.kind),
    ['camera', 'microphone', 'camera+microphone', 'camera', 'camera'],
  )
})

test('each request gets its own digit-only id and is answered on its own', async () => {
  const p = loadPage()
  const first = p.ask({ video: true })
  const second = p.ask({ audio: true })
  const ids = p.sent().map((m) => m.id)
  assert.deepEqual(ids, ['1', '2'])
  assert.ok(ids.every((id) => /^\d+$/.test(id)), 'the native side drops ids that are not all digits')
  p.resolve('2', true)
  assert.equal(await second, p.stream)
  assert.equal(await settled(first), 'pending')
})

test('an unknown or already answered id is ignored', async () => {
  const p = loadPage()
  const pending = p.ask({ video: true })
  assert.doesNotThrow(() => p.resolve('99', true))
  p.resolve('1', true)
  await pending
  p.resolve('1', true)
  assert.equal(p.calls.length, 1)
})

test('without the permission channel every request is denied', async () => {
  const p = loadPage({ channel: null })
  await assert.rejects(p.ask({ video: true }), { name: 'NotAllowedError' })
  assert.equal(p.calls.length, 0)
})

test('a channel that throws denies the request', async () => {
  const p = loadPage()
  p.window.webkit.messageHandlers.wisMediaPermission.postMessage = () => {
    throw new Error('gone')
  }
  await assert.rejects(p.ask({ audio: true }), { name: 'NotAllowedError' })
})

test('a failure in the real getUserMedia reaches the page unchanged', async () => {
  const failure = new Error('OverconstrainedError')
  const p = loadPage({ gum: () => Promise.reject(failure) })
  const pending = p.ask({ video: true })
  p.resolve('1', true)
  await assert.rejects(pending, (err) => err === failure)
})

test('the page cannot replace, delete or enumerate the resolver', () => {
  const p = loadPage()
  const descriptor = Object.getOwnPropertyDescriptor(p.window, '__wisMediaResolve')
  assert.equal(descriptor.writable, false)
  assert.equal(descriptor.configurable, false)
  assert.equal(descriptor.enumerable, false)
})

test('installing twice wraps getUserMedia once', () => {
  const p = loadPage()
  const gate = p.proto.getUserMedia
  p.run()
  assert.equal(p.proto.getUserMedia, gate)
  p.ask({ video: true })
  assert.equal(p.sent().length, 1)
})

test('a page without MediaDevices or getUserMedia is left untouched', () => {
  const bare = loadPage({ media: false })
  assert.equal(bare.window.__wisMediaGate, undefined)
  assert.equal(bare.window.__wisMediaResolve, undefined)
  const noGum = loadPage({ gum: null })
  assert.equal(noGum.window.__wisMediaGate, undefined)
  assert.equal(noGum.proto.getUserMedia, null)
})
