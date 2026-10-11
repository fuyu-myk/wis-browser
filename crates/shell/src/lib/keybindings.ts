/**
 * Mapping between web KeyboardEvent state and Tauri's accelerator
 * string format (see crates/core/src/keybinding.rs for the canonical
 * defaults). Tauri uses `Cmd`, `Ctrl`, `Option`, `Shift` for modifiers
 * and specific named keys (`Left`, `Right`, `Backspace`, `Space`, ...);
 * symbols and letters pass through.
 */

import { isMacOS } from '@/lib/platform'

/** Keys that move the caret or delete within a text field. */
const TEXT_EDITING_KEYS = new Set([
  'ArrowLeft',
  'ArrowRight',
  'ArrowUp',
  'ArrowDown',
  'Home',
  'End',
  'PageUp',
  'PageDown',
  'Backspace',
  'Delete',
])

/** Input types the user types text into. */
const TEXT_INPUT_TYPES = new Set(['text', 'search', 'url', 'tel', 'email', 'password', 'number'])

interface KeystrokeState {
  key: string
  metaKey: boolean
  ctrlKey: boolean
  altKey: boolean
}

/**
 * Whether a keystroke moves the caret or deletes within a text field (Cmd+Left
 * to the line start, Option+Backspace to delete a word), so a shortcut bound to
 * it gives way there. Two of Cmd, Ctrl and Option together edit nothing.
 */
export function isTextEditingKeystroke(e: KeystrokeState): boolean {
  if (!TEXT_EDITING_KEYS.has(e.key)) return false
  return [e.metaKey, e.ctrlKey, e.altKey].filter(Boolean).length <= 1
}

/** Whether `target` is a field the user types text into. */
export function isTextField(target: unknown): boolean {
  const el = target as {
    tagName?: string
    type?: string
    isContentEditable?: boolean
  } | null
  if (!el || typeof el.tagName !== 'string') return false
  if (el.isContentEditable) return true
  const tag = el.tagName.toUpperCase()
  if (tag === 'TEXTAREA') return true
  return tag === 'INPUT' && TEXT_INPUT_TYPES.has((el.type || 'text').toLowerCase())
}

/** True when `name` is a function key (F1..F24). */
function isFunctionKey(name: string): boolean {
  return /^F([1-9]|1[0-9]|2[0-4])$/.test(name)
}

/**
 * Convert a `KeyboardEvent` into a Tauri accelerator string, or `null`
 * when the keystroke can't be bound.
 *
 * Reject unmodified bindings unless the key is a function key (F1–F24).
 */
export function keyboardEventToAccelerator(
  e: KeyboardEvent | React.KeyboardEvent,
): string | null {
  const parts: string[] = []
  if (e.metaKey) parts.push('Cmd')
  if (e.ctrlKey) parts.push('Ctrl')
  if (e.altKey) parts.push('Option')
  if (e.shiftKey) parts.push('Shift')

  const key = e.key
  // Modifier-only presses are not bindable on their own.
  if (
    key === 'Meta' ||
    key === 'Control' ||
    key === 'Alt' ||
    key === 'Shift' ||
    key === 'CapsLock' ||
    key === 'Fn'
  ) {
    return null
  }

  if (key === 'Escape') return null

  let name: string
  switch (key) {
    case 'ArrowLeft':
      name = 'Left'
      break
    case 'ArrowRight':
      name = 'Right'
      break
    case 'ArrowUp':
      name = 'Up'
      break
    case 'ArrowDown':
      name = 'Down'
      break
    case 'Backspace':
      name = 'Backspace'
      break
    case 'Delete':
      name = 'Delete'
      break
    case 'Enter':
      name = 'Enter'
      break
    case 'Tab':
      name = 'Tab'
      break
    case ' ':
      name = 'Space'
      break
    case 'PageUp':
      name = 'PageUp'
      break
    case 'PageDown':
      name = 'PageDown'
      break
    case 'Home':
      name = 'Home'
      break
    case 'End':
      name = 'End'
      break
    default:
      name = key.length === 1 ? key.toUpperCase() : key
  }

  const hasModifier = parts.length > 0
  if (!hasModifier && !isFunctionKey(name)) {
    return null
  }

  parts.push(name)
  return parts.join('+')
}

/**
 * Swap the macOS `Cmd` modifier for `Ctrl` on Windows/Linux, mirroring
 * `localize_accelerator` in crates/core/src/keybinding.rs so this display
 * mirror matches what the native menu actually registers. A no-op on macOS.
 */
export function localizeAccelerator(accel: string): string {
  if (isMacOS()) return accel
  return accel
    .split('+')
    .map((p) => (p === 'Cmd' ? 'Ctrl' : p))
    .join('+')
}

// Modifier sort priority. `Alt` and `Option` are the same physical key (Tauri
// defaults use `Alt`, captured overrides use `Option`); both are listed so a
// modifier is never mistaken for the key part.
const MOD_ORDER: Record<string, number> = {
  Ctrl: 0,
  Option: 1,
  Alt: 1,
  Shift: 2,
  Cmd: 3,
}

// Tokens that prefix an accelerator's key. `Meta` should never reach a binding
// (it is the Windows/Super key) but is listed so it is never mistaken for the
// key part during normalization.
const MODIFIER_TOKENS = new Set(['Cmd', 'Ctrl', 'Option', 'Alt', 'Shift', 'Meta'])

// US-layout shifted symbols folded back to the base key the keymap is authored
// against. A focused webview reports `KeyboardEvent.key` as the shifted glyph
// (Shift+] -> "}"), but the canonical accelerator (and muda's native
// registration) is keyed by the base symbol, so a Shift binding like
// `Ctrl+Shift+]` must compare equal. Layout-specific, matching muda's own
// US-centric default parsing.
const SHIFTED_SYMBOL_BASE: Record<string, string> = {
  '~': '`',
  '!': '1',
  '@': '2',
  '#': '3',
  $: '4',
  '%': '5',
  '^': '6',
  '&': '7',
  '*': '8',
  '(': '9',
  ')': '0',
  _: '-',
  '+': '=',
  '{': '[',
  '}': ']',
  '|': '\\',
  ':': ';',
  '"': "'",
  '<': ',',
  '>': '.',
  '?': '/',
}

/**
 * Canonicalize an accelerator string for equality comparison: unify
 * `Option`/`Alt`, sort the modifiers into a stable order, and (when Shift is
 * held) fold a shifted symbol back to its base key. Applied to both sides of a
 * lookup so a keymap entry like `Alt+Cmd+W` matches a `KeyboardEvent`-derived
 * `Ctrl+Option+W` after the `Cmd`->`Ctrl` host localization, and `Ctrl+Shift+]`
 * matches a keystroke whose `key` came through as `}`.
 */
export function normalizeAccelerator(accel: string): string {
  const mods: string[] = []
  let key = ''
  const parts = accel.split('+')
  if (parts.length > 1 && parts[parts.length - 1] === '' && parts[parts.length - 2] === '') {
    parts.splice(-2, 2, '+')
  }
  for (const part of parts) {
    if (MODIFIER_TOKENS.has(part)) {
      mods.push(part === 'Option' ? 'Alt' : part)
    } else {
      key = part
    }
  }
  if (mods.includes('Shift') && key in SHIFTED_SYMBOL_BASE) {
    key = SHIFTED_SYMBOL_BASE[key]
  }
  if (key.length === 1) key = key.toUpperCase()
  const uniqueSorted = [...new Set(mods)].sort(
    (a, b) => (MOD_ORDER[a] ?? 9) - (MOD_ORDER[b] ?? 9),
  )
  return [...uniqueSorted, key].join('+')
}

// macOS renders accelerators as concatenated glyphs (⇧⌘G); Windows/Linux use
// textual tokens joined by `+` (Ctrl+Shift+G), the native convention there.
const MAC_GLYPHS: Record<string, string> = {
  Cmd: '⌘',
  Shift: '⇧',
  Option: '⌥',
  Alt: '⌥',
  Ctrl: '⌃',
  Left: '←',
  Right: '→',
  Up: '↑',
  Down: '↓',
  Backspace: '⌫',
  Delete: '⌦',
  Enter: '↩',
  Tab: '⇥',
  Space: '␣',
  Escape: '⎋',
}

// Windows/Linux modifier labels. `Cmd` should never reach here (localized
// away) but is mapped defensively.
const PC_LABELS: Record<string, string> = { Cmd: 'Ctrl', Option: 'Alt' }

/**
 * Render a Tauri accelerator string for the pill UI. On macOS: native glyphs
 * in the standard order (Ctrl ⌃, Option ⌥, Shift ⇧, Cmd ⌘, then the key,
 * concatenated). On Windows/Linux: textual tokens joined by `+`.
 */
export function prettyAccelerator(accel: string | null | undefined): string {
  if (!accel) return '—'
  const parts = accel.split('+')
  const modParts = parts
    .filter((p) => p in MOD_ORDER)
    .sort((a, b) => MOD_ORDER[a] - MOD_ORDER[b])
  const keyPart = parts.find((p) => !(p in MOD_ORDER))

  if (isMacOS()) {
    const display = [
      ...modParts.map((p) => MAC_GLYPHS[p] ?? p),
      MAC_GLYPHS[keyPart ?? ''] ?? keyPart ?? '',
    ]
    return display.join('')
  }

  const display = [...modParts.map((p) => PC_LABELS[p] ?? p), keyPart ?? '']
  return display.filter(Boolean).join('+')
}
