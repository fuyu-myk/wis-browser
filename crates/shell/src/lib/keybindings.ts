/**
 * Mapping between web KeyboardEvent state and Tauri's accelerator
 * string format (see crates/core/src/keybinding.rs for the canonical
 * defaults). Tauri uses `Cmd`, `Ctrl`, `Option`, `Shift` for modifiers
 * and specific named keys (`Left`, `Right`, `Backspace`, `Space`, ...);
 * symbols and letters pass through.
 */

import { isMacOS } from '@/lib/platform'

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
