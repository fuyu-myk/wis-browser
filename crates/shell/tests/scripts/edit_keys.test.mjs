// `chrome/edit_keys.rs` EDIT_KEYS_JS: posts Cmd+Z, Cmd+Shift+Z and Cmd+A to `wisEditKey` when the page leaves them unconsumed.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'

const SCRIPT = rawStringConst('chrome/edit_keys.rs', 'EDIT_KEYS_JS')

/** A fresh page running the script. */
function loadPage({ handler = true } = {}) {
  const sent = []
  const listeners = []
  let timers = []
  const window = { addEventListener: (type, fn, capture) => listeners.push({ type, fn, capture }) }
  if (handler) window.webkit = { messageHandlers: { wisEditKey: { postMessage: (cmd) => sent.push(cmd) } } }
  const setTimeout = (fn, ms) => timers.push({ fn, ms })
  const run = () => new Function('window', 'setTimeout', SCRIPT)(window, setTimeout)
  run()

  return {
    sent,
    window,
    listeners,
    reinject: run,
    timers: () => timers,
    /** Dispatches a keydown; the page may claim it any time before the macrotask runs. */
    press(key, mods = {}) {
      const e = { key, isTrusted: true, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, defaultPrevented: false, ...mods }
      listeners.filter((l) => l.type === 'keydown').forEach((l) => l.fn(e))
      return e
    },
    settle() {
      const due = timers
      timers = []
      due.forEach((t) => t.fn())
    },
  }
}

const cmd = { metaKey: true }

describe('installation', () => {
  test('a second injection installs nothing more', () => {
    const p = loadPage()
    p.reinject()
    assert.equal(p.listeners.length, 1)
  })

  test('the watcher listens at bubble phase on the window', () => {
    const p = loadPage()
    assert.deepEqual(
      p.listeners.map((l) => [l.type, l.capture]),
      [['keydown', false]],
    )
  })
})

describe('unconsumed editing keys', () => {
  test('Cmd+Z the page leaves alone posts undo one macrotask after the dispatch', () => {
    const p = loadPage()
    p.press('z', cmd)
    assert.deepEqual(p.sent, [])
    assert.equal(p.timers()[0].ms, 0)
    p.settle()
    assert.deepEqual(p.sent, ['undo'])
  })

  test('a Cmd+Z the page dispatched itself posts nothing', () => {
    const p = loadPage()
    p.press('z', { ...cmd, isTrusted: false })
    p.settle()
    assert.deepEqual(p.sent, [])
  })

  test('Cmd+Shift+Z posts redo', () => {
    const p = loadPage()
    p.press('Z', { metaKey: true, shiftKey: true })
    p.settle()
    assert.deepEqual(p.sent, ['redo'])
  })

  test('Cmd+A posts selectAll', () => {
    const p = loadPage()
    p.press('a', cmd)
    p.settle()
    assert.deepEqual(p.sent, ['selectAll'])
  })

  test('every body is a command name the native handler maps to a selector', () => {
    const p = loadPage()
    p.press('z', cmd)
    p.press('z', { metaKey: true, shiftKey: true })
    p.press('A', cmd)
    p.settle()
    assert.deepEqual(p.sent, ['undo', 'redo', 'selectAll'])
  })

  test('a page that prevents the default, even from a handler that runs later, keeps the key', () => {
    const p = loadPage()
    const e = p.press('z', cmd)
    e.defaultPrevented = true
    p.settle()
    assert.deepEqual(p.sent, [])
  })
})

describe('keys that are left alone', () => {
  const ignored = [
    ['Cmd+Shift+A', 'a', { metaKey: true, shiftKey: true }],
    ['Ctrl+Z', 'z', { ctrlKey: true }],
    ['Cmd+Ctrl+Z', 'z', { metaKey: true, ctrlKey: true }],
    ['Cmd+Option+Z', 'z', { metaKey: true, altKey: true }],
    ['a plain Z', 'z', {}],
    ['Cmd+C', 'c', cmd],
    ['Cmd+Y', 'y', cmd],
    ['a Cmd press with no key', undefined, cmd],
  ]
  for (const [what, key, mods] of ignored) {
    test(`${what} schedules nothing`, () => {
      const p = loadPage()
      p.press(key, mods)
      assert.equal(p.timers().length, 0)
    })
  }
})

describe('the channel', () => {
  test('a missing or throwing handler is tolerated', () => {
    const missing = loadPage({ handler: false })
    missing.press('z', cmd)
    assert.doesNotThrow(() => missing.settle())

    const throwing = loadPage()
    throwing.window.webkit.messageHandlers.wisEditKey.postMessage = () => {
      throw new Error('gone')
    }
    throwing.press('z', cmd)
    assert.doesNotThrow(() => throwing.settle())
  })
})
