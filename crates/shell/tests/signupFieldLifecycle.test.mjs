// What `signup.rs` reports while a password card is anchored to a field:
// focus, scroll and settle, leave, and the ways a card is dismissed.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { rawStringConst } from './support/rustSource.mjs'

const SCRIPT = rawStringConst('signup.rs', 'SIGNUP_JS')

/** A fresh page running the script, with a signup field at y = 300. */
function loadPage() {
  const sent = []
  const listeners = {}
  let timers = new Map()
  let nextTimer = 1
  const window = {
    innerWidth: 1280,
    webkit: { messageHandlers: { wisSignup: { postMessage: (body) => sent.push(body) } } },
    addEventListener: (type, fn) => (listeners[type] ||= []).push(fn),
  }
  window.top = window.self = window
  const document = {
    activeElement: null,
    addEventListener: (type, fn) => (listeners[`doc:${type}`] ||= []).push(fn),
    querySelectorAll: () => ({ length: 1 }),
    querySelector: () => null,
  }
  const setTimeout = (fn) => {
    const id = nextTimer++
    timers.set(id, fn)
    return id
  }
  const clearTimeout = (id) => timers.delete(id)
  new Function('window', 'document', 'setTimeout', 'clearTimeout', SCRIPT)(
    window,
    document,
    setTimeout,
    clearTimeout,
  )

  let top = 300
  const field = {
    type: 'password',
    disabled: false,
    readOnly: false,
    autocomplete: 'new-password',
    maxLength: -1,
    isConnected: true,
    form: null,
    getAttribute: (n) => (n === 'autocomplete' ? 'new-password' : null),
    hasAttribute: () => false,
    getBoundingClientRect: () => ({ left: 100, top, width: 300, height: 40 }),
  }
  const signIn = {
    ...field,
    autocomplete: 'current-password',
    getAttribute: (n) => (n === 'autocomplete' ? 'current-password' : null),
  }
  const fire = (key, event = {}) => (listeners[key] || []).forEach((fn) => fn(event))
  const settle = () => {
    const due = timers
    timers = new Map()
    due.forEach((fn) => fn())
  }

  return {
    sent,
    field,
    signIn,
    window,
    document,
    settle,
    moveTo: (y) => (top = y),
    kinds: () => sent.map((body) => body.kind),
    focus: (target) => fire('doc:focusin', { target }),
    scroll: (target = document) => fire('scroll', { target }),
    resize: () => fire('resize'),
    key: (target, key) => fire('doc:keydown', { target, key }),
    type: (target, isTrusted) => fire('doc:input', { target, isTrusted }),
    click: (target) => fire('doc:pointerdown', { target }),
  }
}

/** Not a sign-in field: no tagName, so nothing about it is reported. */
const username = { type: 'email', getAttribute: () => null }
const holding = (field) => ({ contains: (el) => el === field })

test('focus reports the field box in page pixels with the viewport width', () => {
  const p = loadPage()
  p.focus(p.field)
  assert.deepEqual(p.kinds(), ['focus'])
  assert.equal(p.sent[0].y, 300)
  assert.equal(p.sent[0].viewWidth, 1280)
})

test('a scroll says so at once, then reports the field once when it settles', () => {
  const p = loadPage()
  p.focus(p.field)
  p.moveTo(250)
  p.scroll()
  p.scroll()
  p.scroll()
  assert.deepEqual(p.kinds(), ['focus', 'scroll'])
  p.settle()
  assert.deepEqual(p.kinds(), ['focus', 'scroll', 'move'])
  assert.equal(p.sent.at(-1).y, 250)
})

test('only scrollers that hold the field count, and a resize does', () => {
  const p = loadPage()
  p.focus(p.field)
  p.scroll({ contains: () => false })
  p.settle()
  assert.deepEqual(p.kinds(), ['focus'], 'a carousel scrolling elsewhere')
  p.scroll(holding(p.field))
  p.settle()
  p.resize()
  p.settle()
  assert.deepEqual(p.kinds(), ['focus', 'scroll', 'move', 'scroll', 'move'])
})

test('a release stops the reports and drops a pending settle', () => {
  const p = loadPage()
  p.focus(p.field)
  p.scroll()
  p.window.__wisSignupRelease()
  p.settle()
  p.scroll()
  p.settle()
  assert.deepEqual(p.kinds(), ['focus', 'scroll'])
})

test('a field removed mid-scroll ends its card for good', () => {
  const p = loadPage()
  p.focus(p.field)
  p.scroll()
  p.field.isConnected = false
  p.settle()
  p.scroll()
  p.settle()
  assert.deepEqual(p.kinds(), ['focus', 'scroll', 'leave'])
})

test('focus moving to another field leaves, and a sign-in field reports itself', () => {
  const p = loadPage()
  p.focus(p.field)
  p.focus(username)
  p.scroll()
  p.settle()
  assert.deepEqual(p.kinds(), ['focus', 'leave'], 'the reports stop with it')

  p.focus(p.field)
  p.focus(p.signIn)
  p.scroll()
  p.settle()
  assert.deepEqual(p.kinds().slice(2), ['focus', 'leave', 'login', 'scroll', 'move'])
})

test('refocusing the same field is not a leave', () => {
  const p = loadPage()
  p.focus(p.field)
  p.focus(p.field)
  assert.ok(!p.kinds().includes('leave'))
})

test('Escape in the field dismisses its card, Escape elsewhere does not', () => {
  const p = loadPage()
  p.focus(p.signIn)
  p.key(username, 'Escape')
  assert.deepEqual(p.kinds(), ['login'])
  p.key(p.signIn, 'Escape')
  p.scroll()
  p.settle()
  assert.deepEqual(p.kinds(), ['login', 'leave'])
})

test('a click elsewhere in the page dismisses the card, one in the field does not', () => {
  const p = loadPage()
  p.focus(p.field)
  p.click(p.field)
  assert.deepEqual(p.kinds(), ['focus'])
  p.click({ tagName: 'P' })
  assert.deepEqual(p.kinds(), ['focus', 'leave'])
})

test('typing dismisses the card, a scripted fill does not', () => {
  const p = loadPage()
  p.focus(p.field)
  p.type(p.field, false)
  assert.deepEqual(p.kinds(), ['focus'])
  p.type(p.field, true)
  assert.deepEqual(p.kinds(), ['focus', 'leave'])
})

test('a click on the field that has focus brings its card back', () => {
  const p = loadPage()
  p.focus(p.field)
  p.key(p.field, 'Escape')
  p.document.activeElement = p.field
  p.click(p.field)
  assert.deepEqual(p.kinds(), ['focus', 'leave', 'focus'])
  p.click(p.field)
  assert.deepEqual(p.kinds(), ['focus', 'leave', 'focus'], 'not again while it is up')
})

test('a click on a field without focus waits for its focusin', () => {
  const p = loadPage()
  p.click(p.field)
  assert.deepEqual(p.kinds(), [])
})
