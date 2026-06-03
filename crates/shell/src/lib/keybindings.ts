/**
 * Mapping between web KeyboardEvent state and Tauri's accelerator
 * string format (see crates/core/src/keybinding.rs for the canonical
 * defaults). Tauri uses `Cmd`, `Ctrl`, `Option`, `Shift` for modifiers
 * and specific named keys (`Left`, `Right`, `Backspace`, `Space`, ...);
 * symbols and letters pass through.
 */

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
 * Render a Tauri accelerator string with macOS-native glyphs for the
 * pill UI (e.g. `Cmd+Shift+G` -> `⇧⌘G`). Order is the standard macOS
 * convention: Ctrl ⌃, Option ⌥, Shift ⇧, Cmd ⌘, then the key.
 */
export function prettyAccelerator(accel: string | null | undefined): string {
  if (!accel) return '—'
  const parts = accel.split('+')
  const order: Record<string, number> = { Ctrl: 0, Option: 1, Shift: 2, Cmd: 3 }
  const modParts = parts
    .filter((p) => p in order)
    .sort((a, b) => order[a] - order[b])
  const keyPart = parts.find((p) => !(p in order))

  const glyphs: Record<string, string> = {
    Cmd: '⌘',
    Shift: '⇧',
    Option: '⌥',
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

  const display = [...modParts.map((p) => glyphs[p] ?? p), glyphs[keyPart ?? ''] ?? keyPart ?? '']
  return display.join('')
}
