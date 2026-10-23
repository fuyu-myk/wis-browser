// `chrome/keybind.rs` KEYBIND_JS: forwards bound keystrokes to `wisKeybind` (Windows, Linux, macOS tabs), reserved ones at capture and the rest page-first.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, test } from 'node:test'

import {
  isTextEditingKeystroke,
  isTextField,
  keyboardEventToAccelerator,
  normalizeAccelerator,
} from '../../src/lib/keybindings.ts'
import { rawStringConst } from '../support/rustSource.mjs'

const RAW = rawStringConst('chrome/keybind.rs', 'KEYBIND_JS')

// `reserved_json()`: RESERVED_ACTIONS mapped through `KeyAction::id`.
const KEYBIND_RS = readFileSync(new URL('../../src-tauri/src/chrome/keybind.rs', import.meta.url), 'utf8')
const CORE_RS = readFileSync(new URL('../../../core/src/settings/keybinding.rs', import.meta.url), 'utf8')
const ID_FN = CORE_RS.slice(CORE_RS.indexOf('pub fn id(self)'), CORE_RS.indexOf('pub fn from_id'))
const ID_OF = Object.fromEntries([...ID_FN.matchAll(/Self::(\w+) => "(\w+)"/g)].map((m) => [m[1], m[2]]))
const RESERVED_BLOCK = KEYBIND_RS.slice(KEYBIND_RS.indexOf('const RESERVED_ACTIONS'), KEYBIND_RS.indexOf('];'))
const RESERVED = Object.fromEntries(
  [...RESERVED_BLOCK.matchAll(/KeyAction::(\w+)/g)].map(([, name]) => {
    if (!ID_OF[name]) throw new Error(`no id for KeyAction::${name}`)
    return [ID_OF[name], true]
  }),
)

/** The script as `script()` bakes it; `defer` is the macOS tab variant. */
const scriptFor = (keymap, defer = false) =>
  RAW.replaceAll('__WIS_KEYMAP__', JSON.stringify(keymap))
    .replaceAll('__WIS_RESERVED__', JSON.stringify(RESERVED))
    .replaceAll('__WIS_DEFER__', String(defer))

// A Windows effective keymap: defaults arrive with Cmd localised to Ctrl.
const KEYMAP = {
  new_tab: 'Ctrl+T',
  next_tab: 'Ctrl+Shift+]',
  toggle_sidebar: 'Ctrl+S',
  go_back: 'Alt+Left',
  go_forward: 'Option+Right',
  reload: 'F5',
  find_in_page: 'Ctrl+F',
}

/** A fresh page running the script. */
function loadPage({ keymap = KEYMAP, handler = true, defer = false } = {}) {
  const sent = []
  const listeners = []
  const window = { addEventListener: (type, fn, capture) => listeners.push({ on: 'window', type, fn, capture }) }
  const document = { addEventListener: (type, fn, capture) => listeners.push({ on: 'document', type, fn, capture }) }
  if (handler) window.webkit = { messageHandlers: { wisKeybind: { postMessage: (action) => sent.push(action) } } }
  const run = () => new Function('window', 'document', scriptFor(keymap, defer))(window, document)
  run()

  const phase = (on, capture) => listeners.filter((l) => l.on === on && l.type === 'keydown' && l.capture === capture)
  return {
    sent,
    window,
    listeners,
    reinject: run,
    update: (map) => window.__wisKeybindUpdate(map),
    /**
     * Presses a key through capture on the document, then `page` (the page's
     * own handler), then bubble on the window, honouring stopPropagation.
     */
    press(key, mods = {}, page) {
      const e = {
        key,
        isTrusted: true,
        metaKey: false,
        ctrlKey: false,
        altKey: false,
        shiftKey: false,
        ...mods,
        defaultPrevented: false,
        stopped: false,
        pageSawIt: false,
        preventDefault: () => (e.defaultPrevented = true),
        stopPropagation: () => (e.stopped = true),
      }
      phase('document', true).forEach((l) => l.fn(e))
      if (e.stopped) return e
      e.pageSawIt = true
      page?.(e)
      if (e.stopped) return e
      phase('window', false).forEach((l) => l.fn(e))
      return e
    },
    bubbleOnly(e) {
      phase('window', false).forEach((l) => l.fn(e))
    },
  }
}

const ctrl = { ctrlKey: true }
const ctrlShift = { ctrlKey: true, shiftKey: true }

describe('installation', () => {
  test('a second injection installs nothing more', () => {
    const p = loadPage()
    p.reinject()
    assert.equal(p.listeners.length, 2)
    p.press('t', ctrl)
    assert.deepEqual(p.sent, ['new_tab'])
  })

  test('reserved shortcuts are watched at capture on the document, the rest at bubble on the window', () => {
    const p = loadPage()
    assert.deepEqual(
      p.listeners.map((l) => [l.on, l.type, l.capture]),
      [
        ['document', 'keydown', true],
        ['window', 'keydown', false],
      ],
    )
  })
})

describe('reserved shortcuts', () => {
  test('a reserved shortcut is taken before the page sees it, and posted once', () => {
    const p = loadPage()
    const e = p.press('t', ctrl, (ev) => ev.preventDefault())
    assert.equal(e.pageSawIt, false)
    assert.equal(e.defaultPrevented, true)
    assert.deepEqual(p.sent, ['new_tab'])
  })

  test('a shortcut the page dispatched itself does nothing', () => {
    const p = loadPage()
    const reserved = p.press('t', { ...ctrl, isTrusted: false })
    const plain = p.press('l', { ...ctrl, isTrusted: false })
    assert.deepEqual(p.sent, [])
    assert.ok(!reserved.defaultPrevented && !plain.defaultPrevented)
  })

  test('the bubble listener leaves a reserved shortcut alone', () => {
    const p = loadPage()
    p.bubbleOnly({ key: 't', ctrlKey: true, isTrusted: true, defaultPrevented: false, preventDefault: () => assert.fail('prevented') })
    assert.deepEqual(p.sent, [])
  })
})

describe('page-first shortcuts', () => {
  test('a keystroke the page leaves alone dispatches after it, with its default prevented', () => {
    const p = loadPage()
    let preventedWhenPageRan = null
    const e = p.press('s', ctrl, (ev) => (preventedWhenPageRan = ev.defaultPrevented))
    assert.equal(preventedWhenPageRan, false, 'nothing is taken at capture')
    assert.equal(e.defaultPrevented, true)
    assert.deepEqual(p.sent, ['toggle_sidebar'])
  })

  test('a page that prevents the default keeps the keystroke', () => {
    const p = loadPage()
    p.press('s', ctrl, (ev) => ev.preventDefault())
    assert.deepEqual(p.sent, [])
  })

  test('a page that stops propagation without preventing keeps it too', () => {
    const p = loadPage()
    p.press('s', ctrl, (ev) => ev.stopPropagation())
    assert.deepEqual(p.sent, [])
  })
})

describe('deferred page-first shortcuts (macOS tabs)', () => {
  const MAC = { new_tab: 'Cmd+T', print: 'Cmd+P' }
  const cmd = { metaKey: true }
  const macrotask = () => new Promise((resolve) => setTimeout(resolve, 0))

  test('a keystroke no handler claims posts one macrotask later, default untouched', async () => {
    const p = loadPage({ keymap: MAC, defer: true })
    const e = p.press('p', cmd)
    assert.deepEqual(p.sent, [])
    await macrotask()
    assert.deepEqual(p.sent, ['print'])
    assert.equal(e.defaultPrevented, false)
  })

  test('a page that claims it from a handler running after the watcher keeps it', async () => {
    const p = loadPage({ keymap: MAC, defer: true })
    const e = p.press('p', cmd)
    e.preventDefault()
    await macrotask()
    assert.deepEqual(p.sent, [])
  })

  test('a reserved shortcut is still taken at capture, without waiting', () => {
    const p = loadPage({ keymap: MAC, defer: true })
    const e = p.press('t', cmd)
    assert.equal(e.pageSawIt, false)
    assert.deepEqual(p.sent, ['new_tab'])
  })
})

describe('caret-moving shortcuts in a text field', () => {
  const field = { tagName: 'INPUT', type: 'text' }
  const editing = { ...KEYMAP, go_back: 'Ctrl+Left', focus_pane_left: 'Ctrl+Alt+Left' }

  test('a caret-moving shortcut is left to a focused text field', () => {
    const p = loadPage({ keymap: editing })
    for (const target of [field, { tagName: 'TEXTAREA' }, { tagName: 'DIV', isContentEditable: true }]) {
      const e = p.press('ArrowLeft', { ctrlKey: true, target })
      assert.equal(e.defaultPrevented, false, target.tagName)
    }
    assert.deepEqual(p.sent, [])
  })

  test('outside a text field, or in a field that takes no text, it still runs', () => {
    const p = loadPage({ keymap: editing })
    p.press('ArrowLeft', { ctrlKey: true, target: { tagName: 'DIV' } })
    p.press('ArrowLeft', { ctrlKey: true, target: { tagName: 'INPUT', type: 'checkbox' } })
    assert.deepEqual(p.sent, ['go_back', 'go_back'])
  })

  test('two editing modifiers together edit nothing, so the shortcut runs in a field', () => {
    const p = loadPage({ keymap: editing })
    p.press('ArrowLeft', { ctrlKey: true, altKey: true, target: field })
    assert.deepEqual(p.sent, ['focus_pane_left'])
  })

  test('the deferred variant leaves it to the field too', async () => {
    const p = loadPage({ keymap: { go_back_alt: 'Cmd+Left' }, defer: true })
    p.press('ArrowLeft', { metaKey: true, target: field })
    await new Promise((resolve) => setTimeout(resolve, 0))
    assert.deepEqual(p.sent, [])
  })
})

describe('matching a keystroke to a binding', () => {
  test('an unbound keystroke posts nothing and keeps its default', () => {
    const p = loadPage()
    const e = p.press('k', ctrl)
    assert.equal(e.defaultPrevented, false)
    assert.deepEqual(p.sent, [])
  })

  test('a key without a modifier is not a shortcut, except a function key', () => {
    const p = loadPage()
    p.press('t')
    p.press('F5')
    assert.deepEqual(p.sent, ['reload'])
  })

  test('F1 to F24 count as function keys, F25 does not', () => {
    const p = loadPage()
    p.update({ reload: 'F24', find_in_page: 'F25', print: 'F1' })
    p.press('F24')
    p.press('F25')
    p.press('F1')
    assert.deepEqual(p.sent, ['reload', 'print'])
  })

  test('modifier presses and Escape are never shortcuts', () => {
    const p = loadPage()
    p.update({ find_in_page: 'Ctrl+Escape' })
    for (const key of ['Escape', 'Control', 'Shift', 'Alt', 'Meta', 'CapsLock', 'Fn']) {
      assert.equal(p.press(key, ctrlShift).defaultPrevented, false, key)
    }
    assert.deepEqual(p.sent, [])
  })

  test('letters match in either case, in the keystroke and in the binding', () => {
    const p = loadPage()
    p.update({ toggle_sidebar: 'Ctrl+b' })
    p.press('B', ctrl)
    p.press('b', ctrl)
    assert.deepEqual(p.sent, ['toggle_sidebar', 'toggle_sidebar'])
  })

  test('arrow keys and the space bar use the accelerator names', () => {
    const p = loadPage()
    p.press('ArrowLeft', { altKey: true })
    p.update({ toggle_sidebar: 'Ctrl+Space', find_in_page: 'Ctrl+Up' })
    p.press(' ', ctrl)
    p.press('ArrowUp', ctrl)
    assert.deepEqual(p.sent, ['go_back', 'toggle_sidebar', 'find_in_page'])
  })

  test('other named keys pass through as they are', () => {
    const p = loadPage()
    p.update({ find_in_page: 'Ctrl+PageDown' })
    p.press('PageDown', ctrl)
    assert.deepEqual(p.sent, ['find_in_page'])
  })

  test('Option and Alt are the same modifier', () => {
    const p = loadPage()
    p.press('ArrowLeft', { altKey: true })
    p.press('ArrowRight', { altKey: true })
    assert.deepEqual(p.sent, ['go_back', 'go_forward'])
  })

  test('Meta in a binding is the Cmd modifier the Windows key reports', () => {
    const p = loadPage()
    p.update({ toggle_sidebar: 'Meta+B' })
    p.press('b', { metaKey: true })
    assert.deepEqual(p.sent, ['toggle_sidebar'])
  })

  test('modifier order in a binding does not matter', () => {
    const p = loadPage()
    p.update({ find_in_page: 'Shift+Alt+Ctrl+P' })
    p.press('P', { ctrlKey: true, altKey: true, shiftKey: true })
    assert.deepEqual(p.sent, ['find_in_page'])
  })

  test('a shifted symbol matches its base-key binding, only with Shift held', () => {
    const p = loadPage()
    p.press('}', ctrlShift)
    p.update({ find_in_page: 'Ctrl+]' })
    p.press('}', ctrl)
    assert.deepEqual(p.sent, ['next_tab'])
  })
})

describe('rebinding without a reload', () => {
  test('__wisKeybindUpdate replaces the bindings', () => {
    const p = loadPage()
    p.update({ toggle_sidebar: 'Ctrl+B' })
    p.press('s', ctrl)
    p.press('b', ctrl)
    assert.deepEqual(p.sent, ['toggle_sidebar'])
  })

  test('unbound entries are skipped rather than failing the update', () => {
    const p = loadPage()
    p.update({ toggle_sidebar: 'Ctrl+B', find_in_page: null, reload: '' })
    p.press('b', ctrl)
    p.press('F5')
    assert.deepEqual(p.sent, ['toggle_sidebar'])
  })

  test('inherited properties are not bindings', () => {
    const p = loadPage()
    const map = Object.create({ reload: 'Ctrl+R' })
    map.toggle_sidebar = 'Ctrl+B'
    p.update(map)
    p.press('r', ctrl)
    p.press('b', ctrl)
    assert.deepEqual(p.sent, ['toggle_sidebar'])
  })

  test('a malformed map keeps the previous bindings', () => {
    const p = loadPage()
    assert.doesNotThrow(() => p.update({ toggle_sidebar: 42 }))
    p.press('s', ctrl)
    assert.deepEqual(p.sent, ['toggle_sidebar'])
  })
})

describe('the channel', () => {
  test('the body is the bare action id the dispatcher validates', () => {
    const p = loadPage()
    p.press('t', ctrl)
    p.press('s', ctrl)
    for (const action of p.sent) {
      assert.equal(typeof action, 'string')
      assert.ok(Object.values(ID_OF).includes(action), action)
    }
  })

  test('a missing or throwing handler is tolerated', () => {
    const missing = loadPage({ handler: false })
    assert.doesNotThrow(() => missing.press('t', ctrl))
    const throwing = loadPage()
    throwing.window.webkit.messageHandlers.wisKeybind.postMessage = () => {
      throw new Error('gone')
    }
    assert.doesNotThrow(() => throwing.press('s', ctrl))
  })
})

describe("parity with the chrome's keybindings.ts", () => {
  const SCRIPT = scriptFor({})
  const PAGE = new Function(
    `${SCRIPT.slice(SCRIPT.indexOf('var SHIFT_BASE'), SCRIPT.indexOf('function build'))}
     ${SCRIPT.slice(SCRIPT.indexOf('var NAMED'), SCRIPT.indexOf('function actionOf'))}
     return { normalize, accelOf, textEditing, textField };`,
  )()

  test('normalize agrees with normalizeAccelerator', () => {
    // Meta is left out: the page folds it into Cmd, the chrome keeps it, and
    // keyboardEventToAccelerator never produces it.
    const accelerators = [
      'Ctrl+T',
      'Shift+Ctrl+t',
      'Option+Cmd+W',
      'Alt+Cmd+W',
      'Cmd+Shift+Option+Ctrl+X',
      'Ctrl+Shift+}',
      'Ctrl+}',
      'Ctrl+Shift+?',
      'F5',
      'Ctrl+Left',
      'Ctrl+Space',
      'Cmd+Shift++',
      'Cmd++',
    ]
    for (const accel of accelerators) assert.equal(PAGE.normalize(accel), normalizeAccelerator(accel), accel)
  })

  test('the keystroke mapping agrees with keyboardEventToAccelerator', () => {
    const events = [
      { key: 't', ctrlKey: true },
      { key: '}', ctrlKey: true, shiftKey: true },
      { key: 'ArrowLeft', altKey: true },
      { key: ' ', ctrlKey: true },
      { key: 'Enter', metaKey: true },
      { key: 'F5' },
      { key: 'F25' },
      { key: 'a' },
      { key: 'Escape', ctrlKey: true },
      { key: 'Shift', shiftKey: true },
      { key: 'x', metaKey: true, ctrlKey: true, altKey: true, shiftKey: true },
    ]
    for (const init of events) {
      const e = { metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...init }
      assert.equal(PAGE.accelOf(e), keyboardEventToAccelerator(e), JSON.stringify(init))
    }
  })

  test('the text-field rule agrees with isTextEditingKeystroke and isTextField', () => {
    const keystrokes = [
      { key: 'ArrowLeft', metaKey: true },
      { key: 'ArrowRight', altKey: true },
      { key: 'Backspace', ctrlKey: true },
      { key: 'Home' },
      { key: 'ArrowLeft', metaKey: true, altKey: true },
      { key: 'l', metaKey: true },
    ]
    for (const init of keystrokes) {
      const e = { metaKey: false, ctrlKey: false, altKey: false, ...init }
      assert.equal(PAGE.textEditing(e), isTextEditingKeystroke(e), JSON.stringify(init))
    }
    const targets = [
      { tagName: 'INPUT' },
      { tagName: 'input', type: 'Search' },
      { tagName: 'INPUT', type: 'checkbox' },
      { tagName: 'TEXTAREA' },
      { tagName: 'DIV', isContentEditable: true },
      { tagName: 'DIV' },
      null,
      {},
    ]
    for (const target of targets) {
      assert.equal(PAGE.textField(target), isTextField(target), JSON.stringify(target))
    }
  })
})
