// Which focused fields `vault/signup.rs` treats as a signup, as eligible for a
// generated password, or as a sign-in.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rawStringConst } from './support/rustSource.mjs'

const SCRIPT = rawStringConst('vault/signup.rs', 'SIGNUP_JS')
const HELPERS = SCRIPT.slice(
  SCRIPT.indexOf('function passwordCount'),
  SCRIPT.indexOf("document.addEventListener('focusin'"),
)

/**
 * A page of inputs in document order. Each spec may set `type`,
 * `autocomplete`, `form` (any shared object), `attrs` and `disabled`.
 */
function page(spec) {
  const els = spec.map((s) => ({
    tagName: 'INPUT',
    type: 'text',
    autocomplete: '',
    disabled: false,
    readOnly: false,
    ...s,
  }))
  const inScope = (root) => els.filter((e) => root === doc || e.form === root)
  const select = (root, selector) => {
    let found = inScope(root).filter((e) => e.type === 'password')
    if (selector.includes('current-password')) {
      found = found.filter((e) => e.autocomplete.split(/\s+/).includes('current-password'))
    }
    return found
  }
  const doc = {
    querySelectorAll: (selector) => select(doc, selector),
    querySelector: (selector) => select(doc, selector)[0] || null,
  }
  for (const e of els) {
    e.getAttribute = (n) => (n === 'autocomplete' ? e.autocomplete || null : e.attrs?.[n] ?? null)
    e.hasAttribute = (n) => Boolean(e.attrs && n in e.attrs)
    e.compareDocumentPosition = (other) => (els.indexOf(other) > els.indexOf(e) ? 4 : 2)
    if (e.form) {
      e.form.querySelectorAll = (selector) => select(e.form, selector)
      e.form.querySelector = (selector) => select(e.form, selector)[0] || null
    }
  }
  const fns = new Function('document', `${HELPERS}\nreturn { eligible, qualifies, signIn };`)(doc)
  return { at: (i) => els[i], ...fns }
}

const pw = (extra = {}) => ({ type: 'password', ...extra })

describe('a signup field', () => {
  test('GitHub: a lone autocomplete=off field declaring passwordrules', () => {
    const p = page([pw({ autocomplete: 'off', form: {}, attrs: { passwordrules: 'minlength: 15;' } })])
    assert.equal(p.qualifies(p.at(0)), true)
  })

  test('passwordrules cannot override an explicit current-password', () => {
    const p = page([pw({ autocomplete: 'current-password', form: {}, attrs: { passwordrules: 'x' } })])
    assert.equal(p.qualifies(p.at(0)), false)
  })

  test('Ableton: the register field in its own form beside a sign-in form', () => {
    const login = {}
    const register = {}
    const p = page([pw({ autocomplete: 'current-password', form: login }), pw({ autocomplete: 'off', form: register })])
    assert.equal(p.qualifies(p.at(1)), true)
    assert.equal(p.qualifies(p.at(0)), false, 'the sign-in field beside it must not offer')
  })

  test('a password and its confirm box in one form', () => {
    const form = {}
    const p = page([pw({ form }), pw({ form })])
    assert.equal(p.qualifies(p.at(0)), true)
  })

  test('two fields with no form element, as on a single-page app', () => {
    const p = page([pw(), pw()])
    assert.equal(p.qualifies(p.at(0)), true)
  })

  test('declared new-password', () => {
    const p = page([pw({ autocomplete: 'new-password', form: {} })])
    assert.equal(p.qualifies(p.at(0)), true)
  })

  test('not a plain sign-in, bare or marked', () => {
    for (const autocomplete of ['', 'current-password', 'section-a current-password']) {
      const p = page([pw({ autocomplete, form: {} })])
      assert.equal(p.qualifies(p.at(0)), false, autocomplete || 'bare')
    }
  })
})

describe('the key button', () => {
  test('offers a strong password in a plain unmarked field and a signup one', () => {
    for (const autocomplete of ['', 'new-password']) {
      const p = page([pw({ autocomplete, form: {} })])
      assert.equal(p.eligible(p.at(0)), true, autocomplete || 'bare')
    }
  })

  test('not in a field marked current-password', () => {
    const p = page([pw({ autocomplete: 'current-password', form: {} })])
    assert.equal(p.eligible(p.at(0)), false)
  })
})

describe('a sign-in field', () => {
  test('GitHub login: the username and the password', () => {
    const form = {}
    const p = page([{ autocomplete: 'username', form }, pw({ autocomplete: 'current-password', form })])
    assert.equal(p.signIn(p.at(0)), true)
    assert.equal(p.signIn(p.at(1)), true)
  })

  test('an unmarked email before an unmarked password', () => {
    const form = {}
    const p = page([{ type: 'email', form }, pw({ form })])
    assert.equal(p.signIn(p.at(0)), true)
    assert.equal(p.signIn(p.at(1)), true)
  })

  test('a username-first page: only a field declared a username', () => {
    const declared = page([{ type: 'email', autocomplete: 'username webauthn' }])
    assert.equal(declared.signIn(declared.at(0)), true)
    const bare = page([{ type: 'email' }])
    assert.equal(bare.signIn(bare.at(0)), false, 'a lone undeclared email field is a newsletter box')
  })

  test('not in a signup form, its username included', () => {
    const form = {}
    const p = page([{ type: 'email', autocomplete: 'username', form }, pw({ autocomplete: 'new-password', form })])
    assert.equal(p.signIn(p.at(0)), false)
    assert.equal(p.signIn(p.at(1)), false)
  })

  test('Ableton: the login email but not the register one', () => {
    const login = {}
    const register = {}
    const p = page([
      { type: 'email', form: login },
      pw({ autocomplete: 'current-password', form: login }),
      { type: 'email', form: register },
      pw({ autocomplete: 'off', form: register }),
    ])
    assert.equal(p.signIn(p.at(0)), true)
    assert.equal(p.signIn(p.at(2)), false)
  })

  test('not a text field after the password, nor a search form beside a login form', () => {
    const form = {}
    const after = page([pw({ form }), { form }])
    assert.equal(after.signIn(after.at(1)), false)

    const search = {}
    const login = {}
    const beside = page([{ type: 'search', form: search }, { form: search }, { type: 'email', form: login }, pw({ form: login })])
    assert.equal(beside.signIn(beside.at(1)), false)
  })

  test('not a disabled username, nor a checkbox', () => {
    const form = {}
    const disabled = page([{ autocomplete: 'username', form, disabled: true }, pw({ form })])
    assert.equal(disabled.signIn(disabled.at(0)), false)
    const checkbox = page([{ type: 'checkbox', autocomplete: 'username' }])
    assert.equal(checkbox.signIn(checkbox.at(0)), false)
  })
})
