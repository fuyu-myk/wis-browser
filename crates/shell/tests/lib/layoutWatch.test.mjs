// When an overlay placed against the content host must re-place itself
// (`lib/layoutWatch.ts`).

import assert from 'node:assert/strict'
import { afterEach, test } from 'node:test'

import { watchLayout } from '../../src/lib/layoutWatch.ts'

/** A fake ResizeObserver class whose instances can be made to report. */
function fakeObservers() {
  const made = []
  class Fake {
    constructor(callback) {
      this.callback = callback
      this.observed = []
      this.disconnected = false
      made.push(this)
    }
    observe(el) {
      this.observed.push(el)
    }
    disconnect() {
      this.disconnected = true
    }
  }
  return { Fake, made }
}

afterEach(() => {
  delete globalThis.window
  delete globalThis.ResizeObserver
})

test('a window resize and a change in the element size both report', () => {
  globalThis.window = new EventTarget()
  const { Fake, made } = fakeObservers()
  globalThis.ResizeObserver = Fake
  const host = {}
  let calls = 0
  watchLayout(host, () => calls++)

  window.dispatchEvent(new Event('resize'))
  assert.deepEqual(made[0].observed, [host])
  made[0].callback([])
  assert.equal(calls, 2)
})

test('the cleanup stops both', () => {
  globalThis.window = new EventTarget()
  const { Fake, made } = fakeObservers()
  globalThis.ResizeObserver = Fake
  let calls = 0
  const stop = watchLayout({}, () => calls++)

  stop()
  window.dispatchEvent(new Event('resize'))
  assert.equal(calls, 0)
  assert.equal(made[0].disconnected, true)
})

test('without an element, or without ResizeObserver, the window resize still reports', () => {
  globalThis.window = new EventTarget()
  const { Fake, made } = fakeObservers()
  globalThis.ResizeObserver = Fake
  let calls = 0
  const stopNoElement = watchLayout(null, () => calls++)
  assert.equal(made.length, 0)

  delete globalThis.ResizeObserver
  const stopNoObserver = watchLayout({}, () => calls++)
  window.dispatchEvent(new Event('resize'))
  assert.equal(calls, 2)
  stopNoElement()
  stopNoObserver()
})
