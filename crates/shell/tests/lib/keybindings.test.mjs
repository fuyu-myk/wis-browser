// Keystroke capture, accelerator matching and pill rendering (`lib/keybindings.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import {
  keyboardEventToAccelerator,
  localizeAccelerator,
  normalizeAccelerator,
  prettyAccelerator,
} from '../../src/lib/keybindings.ts'
import { evaluateOn, LINUX_UA, WINDOWS_UA } from '../support/l1-platform.mjs'

// Node's user agent names no OS, which the platform module reads as macOS.
const key = (k, mods = {}) => ({
  key: k,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
})

describe('capturing a keystroke', () => {
  test('modifiers come first in Cmd, Ctrl, Option, Shift order', () => {
    const all = { metaKey: true, ctrlKey: true, altKey: true, shiftKey: true }
    assert.equal(keyboardEventToAccelerator(key('k', all)), 'Cmd+Ctrl+Option+Shift+K')
  })

  test('a letter is upper-cased', () => {
    assert.equal(keyboardEventToAccelerator(key('t', { metaKey: true })), 'Cmd+T')
  })

  test('a symbol passes through as typed', () => {
    assert.equal(keyboardEventToAccelerator(key('[', { metaKey: true })), 'Cmd+[')
    assert.equal(keyboardEventToAccelerator(key('}', { ctrlKey: true, shiftKey: true })), 'Ctrl+Shift+}')
  })

  test('named keys take their accelerator names', () => {
    const cases = {
      ArrowLeft: 'Left',
      ArrowRight: 'Right',
      ArrowUp: 'Up',
      ArrowDown: 'Down',
      Backspace: 'Backspace',
      Delete: 'Delete',
      Enter: 'Enter',
      Tab: 'Tab',
      ' ': 'Space',
      PageUp: 'PageUp',
      PageDown: 'PageDown',
      Home: 'Home',
      End: 'End',
    }
    for (const [pressed, name] of Object.entries(cases)) {
      assert.equal(keyboardEventToAccelerator(key(pressed, { metaKey: true })), `Cmd+${name}`, pressed)
    }
  })

  test('a modifier pressed on its own cannot be bound', () => {
    for (const k of ['Meta', 'Control', 'Alt', 'Shift', 'CapsLock', 'Fn']) {
      assert.equal(keyboardEventToAccelerator(key(k, { metaKey: true, shiftKey: true })), null, k)
    }
  })

  test('Escape is never bindable, even with modifiers', () => {
    assert.equal(keyboardEventToAccelerator(key('Escape', { metaKey: true })), null)
  })

  test('an unmodified key is refused unless it is a function key', () => {
    assert.equal(keyboardEventToAccelerator(key('a')), null)
    assert.equal(keyboardEventToAccelerator(key('ArrowLeft')), null)
    assert.equal(keyboardEventToAccelerator(key('F1')), 'F1')
    assert.equal(keyboardEventToAccelerator(key('F12')), 'F12')
    assert.equal(keyboardEventToAccelerator(key('F24')), 'F24')
  })

  test('only F1 to F24 count as function keys', () => {
    assert.equal(keyboardEventToAccelerator(key('F0')), null)
    assert.equal(keyboardEventToAccelerator(key('F25')), null)
    assert.equal(keyboardEventToAccelerator(key('Fn1')), null)
  })
})

describe('localizing for the host', () => {
  test('macOS keeps Cmd', () => {
    assert.equal(localizeAccelerator('Alt+Cmd+W'), 'Alt+Cmd+W')
  })

  test('Windows and Linux swap Cmd for Ctrl and leave the rest', () => {
    for (const ua of [WINDOWS_UA, LINUX_UA]) {
      const out = evaluateOn(ua, 'keybindings.ts', "[m.localizeAccelerator('Alt+Cmd+W'), m.localizeAccelerator('Shift+F5')]")
      assert.deepEqual(out, ['Alt+Ctrl+W', 'Shift+F5'])
    }
  })
})

describe('matching a keystroke against the keymap', () => {
  test('Option and Alt are the same modifier', () => {
    assert.equal(normalizeAccelerator('Cmd+Option+W'), normalizeAccelerator('Alt+Cmd+W'))
  })

  test('modifier order does not matter', () => {
    assert.equal(normalizeAccelerator('Shift+Cmd+Ctrl+Alt+K'), 'Ctrl+Alt+Shift+Cmd+K')
    assert.equal(normalizeAccelerator('Cmd+Shift+G'), normalizeAccelerator('Shift+Cmd+G'))
  })

  test('a captured Ctrl+Option chord matches a localized Alt+Cmd default', () => {
    const captured = keyboardEventToAccelerator(key('w', { ctrlKey: true, altKey: true }))
    assert.equal(normalizeAccelerator(captured), normalizeAccelerator('Alt+Ctrl+W'))
  })

  test('with Shift held, a shifted symbol folds back to its base key', () => {
    assert.equal(normalizeAccelerator('Ctrl+Shift+}'), normalizeAccelerator('Ctrl+Shift+]'))
    assert.equal(normalizeAccelerator('Cmd+Shift+{'), 'Shift+Cmd+[')
    assert.equal(normalizeAccelerator('Cmd+Shift+?'), 'Shift+Cmd+/')
    assert.equal(normalizeAccelerator('Cmd+Shift+_'), 'Shift+Cmd+-')
  })

  test('without Shift a symbol is left alone', () => {
    assert.equal(normalizeAccelerator('Cmd+}'), 'Cmd+}')
  })

  test('a letter key compares case-insensitively', () => {
    assert.equal(normalizeAccelerator('Cmd+t'), 'Cmd+T')
  })

  test('named keys keep their spelling', () => {
    assert.equal(normalizeAccelerator('Alt+Cmd+Left'), 'Alt+Cmd+Left')
  })

  test('a repeated modifier counts once', () => {
    assert.equal(normalizeAccelerator('Option+Alt+Cmd+W'), 'Alt+Cmd+W')
  })

  test('Meta is kept as a modifier, never taken for the key', () => {
    assert.equal(normalizeAccelerator('Meta+Ctrl+K'), 'Ctrl+Meta+K')
  })

  test("Shift with the '+' glyph folds back to the '=' key", () => {
    assert.equal(normalizeAccelerator('Cmd+Shift++'), 'Shift+Cmd+=')
  })

  test("a '+' key without Shift stays the '+' key", () => {
    assert.equal(normalizeAccelerator('Cmd++'), 'Cmd++')
  })
})

describe('rendering the pill', () => {
  test('no binding renders as a dash', () => {
    assert.equal(prettyAccelerator(null), '—')
    assert.equal(prettyAccelerator(undefined), '—')
    assert.equal(prettyAccelerator(''), '—')
  })

  test('macOS shows glyphs in Ctrl, Option, Shift, Cmd order', () => {
    assert.equal(prettyAccelerator('Cmd+Shift+G'), '⇧⌘G')
    assert.equal(prettyAccelerator('Cmd+Shift+Ctrl+Option+K'), '⌃⌥⇧⌘K')
    assert.equal(prettyAccelerator('Alt+Cmd+Left'), '⌥⌘←')
  })

  test('macOS shows named keys as glyphs and others as written', () => {
    assert.equal(prettyAccelerator('Cmd+Backspace'), '⌘⌫')
    assert.equal(prettyAccelerator('Escape'), '⎋')
    assert.equal(prettyAccelerator('F5'), 'F5')
  })

  test('Windows and Linux show textual modifiers joined by +', () => {
    const out = evaluateOn(
      WINDOWS_UA,
      'keybindings.ts',
      "[m.prettyAccelerator('Shift+Ctrl+G'), m.prettyAccelerator('Option+Ctrl+Left'), m.prettyAccelerator('Cmd+T'), m.prettyAccelerator('F5'), m.prettyAccelerator(null)]",
    )
    assert.deepEqual(out, ['Ctrl+Shift+G', 'Ctrl+Alt+Left', 'Ctrl+T', 'F5', '—'])
  })
})
