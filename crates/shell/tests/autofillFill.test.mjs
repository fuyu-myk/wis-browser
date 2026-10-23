// Where `vault/passwords.rs`'s fill script writes a saved login.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { formatTemplate } from './support/rustSource.mjs'

const SCRIPT = formatTemplate('vault/passwords.rs', 'fill_script', {
  host: JSON.stringify('example.com'),
  user: JSON.stringify('alice'),
  pass: JSON.stringify('secret'),
})

/** Run the fill over inputs in document order, `active` focused; returns their values. */
function fill(spec, active, hostname = 'example.com') {
  const els = spec.map((s) => ({
    tagName: 'INPUT',
    type: 'text',
    autocomplete: '',
    disabled: false,
    readOnly: false,
    value: '',
    ...s,
  }))
  const inScope = (root) => els.filter((e) => root === doc || e.form === root)
  const select = (root, selector) =>
    selector.startsWith('input[type=password]')
      ? inScope(root).filter((e) => e.type === 'password' && !e.disabled && !e.readOnly)
      : inScope(root)
  const doc = {
    activeElement: active == null ? { tagName: 'BODY' } : els[active],
    querySelector: (selector) => select(doc, selector)[0] || null,
    querySelectorAll: (selector) => select(doc, selector),
  }
  for (const e of els) {
    e.getAttribute = (n) => (n === 'autocomplete' ? e.autocomplete || null : null)
    e.compareDocumentPosition = (other) => (els.indexOf(other) > els.indexOf(e) ? 4 : 2)
    e.dispatchEvent = () => true
    if (e.form) e.form.querySelector = (selector) => select(e.form, selector)[0] || null
  }
  class Event {
    constructor(type) {
      this.type = type
    }
  }
  new Function('document', 'Event', 'location', SCRIPT)(doc, Event, { hostname })
  return els.map((e) => e.value)
}

test('a focused username fills it and its password', () => {
  const form = {}
  assert.deepEqual(fill([{ autocomplete: 'username', form }, { type: 'password', form }], 0), ['alice', 'secret'])
})

test('a focused password fills it and the username before it', () => {
  const form = {}
  assert.deepEqual(fill([{ type: 'email', form }, { type: 'password', form }], 1), ['alice', 'secret'])
})

test('a username-first page fills the username alone', () => {
  assert.deepEqual(fill([{ type: 'email', autocomplete: 'username webauthn' }], 0), ['alice'])
})

test('a page with two forms fills the one in use, not the first', () => {
  const register = {}
  const login = {}
  const values = fill(
    [
      { type: 'email', form: register },
      { type: 'password', form: register },
      { type: 'email', form: login },
      { type: 'password', autocomplete: 'current-password', form: login },
    ],
    2,
  )
  assert.deepEqual(values, ['', '', 'alice', 'secret'])
})

test('with nothing focused, the first password and the username before it, as the key button always did', () => {
  const form = {}
  assert.deepEqual(fill([{ form }, { type: 'password', form }], null), ['alice', 'secret'])
})

test('a focused search box is not taken for the username', () => {
  const form = {}
  const search = {}
  assert.deepEqual(fill([{ form }, { type: 'password', form }, { form: search }], 2), ['alice', 'secret', ''])
})

test('no login fields: nothing is written', () => {
  assert.deepEqual(fill([{ type: 'search' }], 0), [''])
})

test('a page that has moved to another site by the time the fill runs gets nothing', () => {
  const form = {}
  const login = [{ autocomplete: 'username', form }, { type: 'password', form }]
  assert.deepEqual(fill(login, 0, 'evil.test'), ['', ''])
  assert.deepEqual(fill(login, 0, 'example.com.evil.test'), ['', ''])
  assert.deepEqual(fill(login, 0, 'www.example.com'), ['alice', 'secret'])
})

test('a page that patches string methods still cannot pass the site check', () => {
  const { replace, startsWith, endsWith, slice } = String.prototype
  try {
    String.prototype.replace = () => 'example.com'
    String.prototype.startsWith = () => true
    String.prototype.endsWith = () => true
    String.prototype.slice = () => 'example.com'
    const form = {}
    assert.deepEqual(fill([{ autocomplete: 'username', form }, { type: 'password', form }], 0, 'evil.test'), ['', ''])
  } finally {
    Object.assign(String.prototype, { replace, startsWith, endsWith, slice })
  }
})
