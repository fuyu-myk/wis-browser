/**
 * Arrow-key focus movement inside a menu, list or group: the arrows step
 * through its items (wrapping), Home and End jump to the ends, and disabled
 * items are skipped.
 */

export type ArrowOrientation = 'vertical' | 'horizontal' | 'both'

const FORWARD: Record<ArrowOrientation, readonly string[]> = {
  vertical: ['ArrowDown'],
  horizontal: ['ArrowRight'],
  both: ['ArrowDown', 'ArrowRight'],
}
const BACK: Record<ArrowOrientation, readonly string[]> = {
  vertical: ['ArrowUp'],
  horizontal: ['ArrowLeft'],
  both: ['ArrowUp', 'ArrowLeft'],
}

/** The item `key` moves to from `current` (-1 when none), or null when it does not move. */
export function arrowStep(
  key: string,
  orientation: ArrowOrientation,
  current: number,
  disabled: readonly boolean[],
): number | null {
  const count = disabled.length
  const enabled = (i: number) => !disabled[i]
  const scan = (from: number, delta: number): number | null => {
    for (let n = 1; n <= count; n += 1) {
      const i = (((from + delta * n) % count) + count) % count
      if (enabled(i)) return i
    }
    return null
  }
  if (count === 0) return null
  if (key === 'Home') return scan(-1, 1)
  if (key === 'End') return scan(count, -1)
  if (FORWARD[orientation].includes(key)) return scan(current < 0 ? -1 : current, 1)
  if (BACK[orientation].includes(key)) return scan(current < 0 ? count : current, -1)
  return null
}

function isDisabled(el: HTMLElement): boolean {
  return el.matches(':disabled') || el.getAttribute('aria-disabled') === 'true'
}

interface ArrowKeyEvent {
  key: string
  defaultPrevented: boolean
  altKey: boolean
  ctrlKey: boolean
  metaKey: boolean
  preventDefault(): void
}

/**
 * Move focus among `root`'s `items` for an arrow, Home or End press. Returns
 * whether it moved. Keys pressed in a field (text, select, editable) are left
 * to it; Tab leaves the field.
 */
export function moveFocusByArrow(
  event: ArrowKeyEvent,
  root: HTMLElement | null,
  items: string,
  orientation: ArrowOrientation = 'vertical',
): boolean {
  if (!root || event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return false
  const active = document.activeElement
  const typing =
    active instanceof HTMLInputElement ||
    active instanceof HTMLTextAreaElement ||
    active instanceof HTMLSelectElement ||
    (active instanceof HTMLElement && active.isContentEditable)
  if (typing) return false
  const list = [...root.querySelectorAll<HTMLElement>(items)]
  const current = list.findIndex((el) => el === active || el.contains(active))
  const next = arrowStep(event.key, orientation, current, list.map(isDisabled))
  if (next === null) return false
  event.preventDefault()
  list[next].focus()
  return true
}
