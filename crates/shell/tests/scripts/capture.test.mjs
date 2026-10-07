// `capture.rs` CAPTURE_JS: spots a submitted login and posts `{ username, password }` to `wisCapture`.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'

const SCRIPT = rawStringConst('capture.rs', 'CAPTURE_JS')

/** An `<input>` the script can read. `attrs` holds what `getAttribute` returns. */
function input(spec = {}) {
  const el = { tagName: 'INPUT', type: 'text', value: '', name: '', id: '', autocomplete: '', ...spec }
  el.getAttribute = (n) => spec.attrs?.[n] ?? null
  return el
}

/** A clickable element; `attrs` holds `type`, `role`, `aria-label`, `data-a-target`. */
function control(tag, spec = {}) {
  return {
    tagName: tag.toUpperCase(),
    textContent: '',
    ...spec,
    getAttribute: (n) => spec.attrs?.[n] ?? null,
  }
}

const matches = (el, selector) => {
  if (selector === '*') return true
  if (el.tagName !== 'INPUT') return false
  if (selector === 'input') return true
  if (selector === 'input[type=password]') return (el.type || '').toLowerCase() === 'password'
  throw new Error(`unexpected selector ${selector}`)
}

/** A node list root: the document itself or an open shadow root. */
function scope(nodes) {
  return { nodes, querySelectorAll: (selector) => nodes.filter((el) => matches(el, selector)) }
}

/** A fresh page running the script. `nodes` is the document's elements in order. */
function loadPage({ handler = true, observer = true, nodes = [], rootElement = { nodeName: 'HTML' } } = {}) {
  const sent = []
  const listeners = {}
  const observers = []
  let timers = []
  const window = {
    addEventListener: (type, fn) => (listeners[`window:${type}`] ||= []).push(fn),
  }
  if (handler) {
    window.webkit = { messageHandlers: { wisCapture: { postMessage: (body) => sent.push(body) } } }
  }
  const root = scope(nodes)
  const document = {
    documentElement: rootElement,
    querySelectorAll: root.querySelectorAll,
    addEventListener: (type, fn) => (listeners[type] ||= []).push(fn),
  }
  class MutationObserver {
    constructor(callback) {
      this.callback = callback
      observers.push(this)
    }
    observe(target, options) {
      // As the DOM does for a target that is not a Node.
      if (!target) throw new TypeError('not a Node')
      this.target = target
      this.options = options
    }
  }
  const setTimeout = (fn, ms) => timers.push({ fn, ms })
  const run = () =>
    new Function('window', 'document', 'setTimeout', 'MutationObserver', SCRIPT)(
      window,
      document,
      setTimeout,
      observer ? MutationObserver : undefined,
    )
  run()

  const fire = (key, event) => (listeners[key] || []).forEach((fn) => fn(event))
  const pathOf = (el, extra = []) => [el, ...extra, document, window]
  const p = {
    sent,
    window,
    document,
    observers,
    listeners,
    timers: () => timers,
    reinject: run,
    add: (el) => nodes.push(el),
    remove: (el) => nodes.splice(nodes.indexOf(el), 1),
    /** Types `value` into `el` as the user would. */
    type(el, value) {
      el.value = value
      fire('input', { target: el, composedPath: () => pathOf(el) })
    },
    submit: () => fire('submit', { target: {} }),
    key: (el, key, extra = []) => fire('keydown', { key, target: el, composedPath: () => pathOf(el, extra) }),
    click: (el, extra = []) => fire('click', { target: el, composedPath: () => pathOf(el, extra) }),
    pagehide: () => fire('window:pagehide', {}),
    mutate: () => observers.forEach((o) => o.callback([])),
    settle() {
      const due = timers
      timers = []
      due.forEach((t) => t.fn())
    },
  }
  return p
}

/** A page with a username and a password field, both filled. */
function loginPage(options = {}) {
  const user = input({ type: 'email', name: 'email', value: 'alice@example.com' })
  const pass = input({ type: 'password', name: 'password', value: 'hunter2' })
  const p = loadPage({ ...options, nodes: [user, pass] })
  return { ...p, user, pass }
}

describe('installation', () => {
  test('a second injection into the same page adds no second set of listeners', () => {
    const p = loginPage()
    p.reinject()
    assert.equal(p.listeners.submit.length, 1)
    p.submit()
    assert.equal(p.sent.length, 1)
  })

  test('a page without the wisCapture handler, as in a private tab, posts nothing and does not throw', () => {
    const p = loginPage({ handler: false })
    assert.doesNotThrow(() => p.submit())
    assert.deepEqual(p.sent, [])
  })

  test('a handler that throws is tolerated', () => {
    const p = loginPage()
    p.window.webkit.messageHandlers.wisCapture.postMessage = () => {
      throw new Error('gone')
    }
    assert.doesNotThrow(() => p.submit())
  })

  test('without MutationObserver the other signals still work', () => {
    const p = loginPage({ observer: false })
    assert.equal(p.observers.length, 0)
    p.submit()
    assert.equal(p.sent.length, 1)
  })

  test('with no root element to observe yet, the other signals still work', () => {
    const p = loginPage({ rootElement: null })
    assert.equal(p.observers[0].target, undefined)
    p.submit()
    assert.equal(p.sent.length, 1)
  })

  test('the observer watches the whole document for added and removed nodes', () => {
    const p = loginPage()
    assert.equal(p.observers[0].target, p.document.documentElement)
    assert.deepEqual(p.observers[0].options, { childList: true, subtree: true })
  })
})

describe('what is posted', () => {
  test('a form submit posts exactly the username and password the native handler reads', () => {
    const p = loginPage()
    p.submit()
    assert.deepEqual(p.sent, [{ username: 'alice@example.com', password: 'hunter2' }])
  })

  test('nothing is posted when no password was filled or typed', () => {
    const p = loadPage({ nodes: [input({ type: 'email', value: 'a@b.co' }), input({ type: 'password' })] })
    p.submit()
    p.pagehide()
    assert.deepEqual(p.sent, [])
  })

  test('the same login is posted once however many signals fire', () => {
    const p = loginPage()
    p.submit()
    p.key(p.pass, 'Enter')
    p.pagehide()
    assert.equal(p.sent.length, 1)
  })

  test('a changed password is posted again', () => {
    const p = loginPage()
    p.submit()
    p.pass.value = 'hunter3'
    p.submit()
    assert.deepEqual(
      p.sent.map((b) => b.password),
      ['hunter2', 'hunter3'],
    )
  })

  test('an empty password field is passed over for a filled one', () => {
    const p = loadPage({ nodes: [input({ type: 'password' }), input({ type: 'password', value: 'second' })] })
    p.submit()
    assert.equal(p.sent[0].password, 'second')
  })

  test('fields inside open shadow roots are found', () => {
    const inner = scope([input({ type: 'email', value: 'shadow@example.com' }), input({ type: 'password', value: 'pw' })])
    const host = { tagName: 'LOGIN-FORM', shadowRoot: inner }
    const p = loadPage({ nodes: [host] })
    p.submit()
    assert.deepEqual(p.sent, [{ username: 'shadow@example.com', password: 'pw' }])
  })

  test('a root whose querySelectorAll throws is treated as empty', () => {
    const broken = {
      querySelectorAll: () => {
        throw new Error('closed')
      },
    }
    const p = loadPage({ nodes: [{ tagName: 'X-WIDGET', shadowRoot: broken }, input({ type: 'password', value: 'pw' })] })
    assert.doesNotThrow(() => p.submit())
    assert.equal(p.sent[0].password, 'pw')
  })
})

describe('which username is reported', () => {
  const usernameOf = (nodes) => {
    const p = loadPage({ nodes: [...nodes, input({ type: 'password', value: 'pw' })] })
    p.submit()
    return p.sent[0].username
  }

  test('an email field wins over an earlier unrelated text field', () => {
    assert.equal(usernameOf([input({ name: 'q', value: 'shoes' }), input({ type: 'email', value: 'a@b.co' })]), 'a@b.co')
  })

  test('a text field whose name, id, autocomplete or aria-label reads like a username is used', () => {
    assert.equal(usernameOf([input({ name: 'q', value: 'x' }), input({ name: 'login_id', value: 'alice' })]), 'alice')
    assert.equal(usernameOf([input({ id: 'UserName', value: 'bob' })]), 'bob')
    assert.equal(usernameOf([input({ autocomplete: 'username', value: 'carol' })]), 'carol')
    assert.equal(usernameOf([input({ type: 'tel', attrs: { 'aria-label': 'Phone number' }, value: '555' })]), '555')
  })

  test('a hidden field named username is skipped in favour of a visible one', () => {
    assert.equal(
      usernameOf([input({ type: 'hidden', name: 'username', value: 'stale' }), input({ name: 'nick', value: 'visible' })]),
      'visible',
    )
  })

  test('an email-shaped value is preferred to the first filled text field', () => {
    assert.equal(usernameOf([input({ name: 'nick', value: 'xyz' }), input({ name: 'f2', value: 'bob@x.io' })]), 'bob@x.io')
  })

  test('an email kept in a hidden field is used on a password-only step', () => {
    assert.equal(usernameOf([input({ type: 'hidden', name: 'identifier', value: 'bob@x.io' })]), 'bob@x.io')
  })

  test('otherwise the first filled text, email or tel field is used', () => {
    assert.equal(usernameOf([input({ type: 'checkbox', value: 'on' }), input({ name: 'nick', value: 'xyz' })]), 'xyz')
  })

  test('with no field to read, the last username typed on the page is used', () => {
    const user = input({ name: 'username' })
    const pass = input({ type: 'password', value: 'pw' })
    const p = loadPage({ nodes: [user, pass] })
    p.type(user, 'alice')
    user.value = ''
    p.submit()
    assert.equal(p.sent[0].username, 'alice')
  })

  test('typing into a field that does not read like a username is not remembered', () => {
    const search = input({ name: 'q' })
    const pass = input({ type: 'password', value: 'pw' })
    const p = loadPage({ nodes: [search, pass] })
    p.type(search, 'shoes')
    p.remove(search)
    p.submit()
    assert.equal(p.sent[0].username, '')
  })

  test('a login with no username at all posts an empty one, which the native side accepts', () => {
    assert.equal(usernameOf([]), '')
  })

  test('a password shaped like an email is never reported as the username', () => {
    const pass = input({ type: 'password', value: 'me@mail.example' })
    const p = loadPage({ nodes: [pass] })
    p.submit()
    assert.equal(p.sent[0].password, 'me@mail.example')
    assert.equal(p.sent[0].username, '')
  })

  test('an email held in a hidden field still names the account on a password-only step', () => {
    assert.equal(usernameOf([input({ type: 'hidden', value: 'me@mail.example' })]), 'me@mail.example')
  })
})

describe('signals that a login was submitted', () => {
  test('Enter in a filled password field posts', () => {
    const p = loginPage()
    p.key(p.pass, 'Enter')
    assert.equal(p.sent.length, 1)
  })

  test('Enter in an empty password field, Enter elsewhere, or any other key posts nothing', () => {
    const p = loginPage()
    p.key(p.pass, 'a')
    p.key(p.user, 'Enter')
    p.pass.value = ''
    p.key(p.pass, 'Enter')
    assert.deepEqual(p.sent, [])
  })

  test('Enter in a password field inside a shadow root is seen through composedPath', () => {
    const pass = input({ type: 'password', value: 'pw' })
    const host = { tagName: 'LOGIN-FORM', shadowRoot: scope([pass]) }
    const p = loadPage({ nodes: [host] })
    p.key(pass, 'Enter', [host])
    assert.equal(p.sent.length, 1)
  })

  test('without composedPath the event target is read', () => {
    const p = loginPage()
    for (const fn of p.listeners.keydown) fn({ key: 'Enter', target: p.pass })
    assert.equal(p.sent.length, 1)
  })

  test('page teardown posts', () => {
    const p = loginPage()
    p.pagehide()
    assert.equal(p.sent.length, 1)
  })

  test('a click on a sign-in control posts, found from an inner icon', () => {
    const p = loginPage()
    const button = control('button', { textContent: '  Sign in  ' })
    p.click(control('span'), [button])
    assert.equal(p.sent.length, 1)
  })

  test('a click on a submit control with no password anywhere posts nothing', () => {
    const p = loadPage({ nodes: [input({ type: 'email', value: 'a@b.co' })] })
    p.click(control('button', { attrs: { type: 'submit' } }))
    assert.deepEqual(p.sent, [])
  })

  test('a click on a submit control posts the typed password after its field is gone', () => {
    const pass = input({ type: 'password' })
    const p = loadPage({ nodes: [input({ type: 'email', value: 'a@b.co' }), pass] })
    p.type(pass, 'typed')
    p.remove(pass)
    p.click(control('button', { textContent: 'Continue' }))
    assert.deepEqual(p.sent, [{ username: 'a@b.co', password: 'typed' }])
  })

  test('a click on an ordinary element posts nothing', () => {
    const p = loginPage()
    p.click(control('div', { textContent: 'Sign in to see more' }), [control('section')])
    assert.deepEqual(p.sent, [])
  })

  const submitLike = [
    ['an input of type submit', control('input', { attrs: { type: 'submit' } }), true],
    ['an input of type button', control('input', { attrs: { type: 'button' } }), true],
    ['a button of type submit with any label', control('button', { textContent: 'Go', attrs: { type: 'submit' } }), true],
    ['a "Log in" button', control('button', { textContent: 'Log in' }), true],
    ['a "Next" button', control('button', { textContent: 'Next' }), true],
    ['a "Create an account" link', control('a', { textContent: 'Create an account' }), true],
    ['a role=button labelled by aria-label', control('div', { attrs: { role: 'button', 'aria-label': 'Sign in' } }), true],
    ['a button named by data-a-target', control('button', { attrs: { 'data-a-target': 'passport-login-button' } }), true],
    ['a button whose value says Submit', control('button', { value: 'Submit' }), true],
    ['an input of type text', control('input', { attrs: { type: 'text' } }), false],
    ['a "Cancel" button', control('button', { textContent: 'Cancel' }), false],
    ['a span reading Sign in without a button role', control('span', { textContent: 'Sign in' }), false],
  ]
  for (const [what, el, posts] of submitLike) {
    test(`a click on ${what} ${posts ? 'posts' : 'posts nothing'}`, () => {
      const p = loginPage()
      p.click(el)
      assert.equal(p.sent.length, posts ? 1 : 0)
    })
  }

  test('path entries without a tag name are passed over', () => {
    const p = loginPage()
    p.click({ nodeType: 3 }, [control('button', { textContent: 'Log in' })])
    assert.equal(p.sent.length, 1)
  })

  test('an input event with no INPUT in its path is ignored', () => {
    const p = loadPage({ nodes: [] })
    for (const fn of p.listeners.input) fn({ target: control('div') })
    p.pagehide()
    assert.deepEqual(p.sent, [])
  })
})

describe('the password field disappearing', () => {
  /** A typed login whose password field then leaves the page. */
  function typedThenRemoved() {
    const user = input({ type: 'email', value: 'a@b.co' })
    const pass = input({ type: 'password' })
    const p = loadPage({ nodes: [user, pass] })
    p.type(pass, 'typed')
    return { ...p, pass }
  }

  test('removing the filled password field posts once the DOM settles', () => {
    const p = typedThenRemoved()
    p.remove(p.pass)
    p.mutate()
    assert.deepEqual(p.sent, [])
    assert.equal(p.timers()[0].ms, 200)
    p.settle()
    assert.deepEqual(p.sent, [{ username: 'a@b.co', password: 'typed' }])
  })

  test('a burst of mutations schedules one check', () => {
    const p = typedThenRemoved()
    p.mutate()
    p.mutate()
    p.mutate()
    assert.equal(p.timers().length, 1)
  })

  test('nothing is checked before a password was typed', () => {
    const p = loginPage()
    p.mutate()
    assert.equal(p.timers().length, 0)
  })

  test('a password field still present posts nothing, and a later mutation checks again', () => {
    const p = typedThenRemoved()
    p.mutate()
    p.settle()
    assert.deepEqual(p.sent, [])
    p.remove(p.pass)
    p.mutate()
    p.settle()
    assert.equal(p.sent.length, 1)
  })

  test('after it posts, further mutations schedule nothing until a password is typed again', () => {
    const p = typedThenRemoved()
    p.remove(p.pass)
    p.mutate()
    p.settle()
    p.mutate()
    assert.equal(p.timers().length, 0)
  })

  test('typing into an empty password field does not arm the check', () => {
    const pass = input({ type: 'password' })
    const p = loadPage({ nodes: [pass] })
    p.type(pass, '')
    p.mutate()
    assert.equal(p.timers().length, 0)
  })
})
