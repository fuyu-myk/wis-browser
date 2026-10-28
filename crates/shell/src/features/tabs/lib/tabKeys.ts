/**
 * Keyboard use of the tab strips: Enter or Space switches to a tab, Delete or
 * Backspace closes it, Shift+F10 or the menu key opens its menu, and the
 * arrows move between tabs. Only the active tab is a Tab stop.
 */

export type TabKeyAction = 'activate' | 'close' | 'menu'

interface TabKey {
  key: string
  shiftKey: boolean
  altKey: boolean
  ctrlKey: boolean
  metaKey: boolean
}

export function tabKeyAction(e: TabKey): TabKeyAction | null {
  if (e.altKey || e.ctrlKey || e.metaKey) return null
  if (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) return 'menu'
  if (e.shiftKey) return null
  if (e.key === 'Enter' || e.key === ' ') return 'activate'
  if (e.key === 'Delete' || e.key === 'Backspace') return 'close'
  return null
}

/** What focus moves to when `closing` closes: the next stop, else the previous. */
export function neighbourAfterClose<T>(stops: readonly T[], closing: T): T | null {
  const at = stops.indexOf(closing)
  if (at < 0) return null
  return stops[at + 1] ?? stops[at - 1] ?? null
}

/**
 * The strip's focusable stops around `row`, in order: shown tabs and folder
 * headers, never a row inside a collapsed folder or one already closing.
 */
function stripStops(row: HTMLElement): HTMLElement[] {
  const strip = row.closest('[data-tab-strip]')
  if (!strip) return []
  return [...strip.querySelectorAll<HTMLElement>('[data-tab-row], .kb-tabfolder-head, .h-island-name')].filter(
    (el) =>
      el === row ||
      (!el.classList.contains('is-closing') &&
        !el.closest('.kb-foldwrap:not(.is-open)') &&
        el.getClientRects().length > 0),
  )
}

/** If focus is in `row`, move it to the stop that takes its place. */
export function focusNeighbour(row: HTMLElement | null): void {
  if (!row || !row.contains(document.activeElement)) return
  neighbourAfterClose(stripStops(row), row)?.focus()
}

/** Where a keyboard-opened menu appears. */
export interface MenuPoint {
  clientX: number
  clientY: number
  preventDefault(): void
}

export function menuPointFor(row: HTMLElement): MenuPoint {
  const rect = row.getBoundingClientRect()
  return { clientX: rect.left + 12, clientY: rect.bottom, preventDefault: () => undefined }
}

interface TabRowHandlers {
  activate: () => void
  /** Absent where the key does not close (pinned tabs). */
  close?: () => void
  menu: (at: MenuPoint) => void
}

/** The row's own keys; the arrows are the strip's (`moveFocusByArrow`). */
export function onTabRowKey(
  e: TabKey & { currentTarget: HTMLElement; target: EventTarget; preventDefault(): void },
  handlers: TabRowHandlers,
): void {
  if (e.target !== e.currentTarget) return
  const action = tabKeyAction(e)
  if (!action || (action === 'close' && !handlers.close)) return
  e.preventDefault()
  if (action === 'activate') handlers.activate()
  else if (action === 'menu') handlers.menu(menuPointFor(e.currentTarget))
  else {
    focusNeighbour(e.currentTarget)
    handlers.close?.()
  }
}

/** Alt with the strip's own arrows moves the focused item. */
export function moveDelta(e: TabKey, orientation: 'vertical' | 'horizontal'): -1 | 1 | null {
  if (!e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return null
  const [back, on] = orientation === 'vertical' ? ['ArrowUp', 'ArrowDown'] : ['ArrowLeft', 'ArrowRight']
  return e.key === back ? -1 : e.key === on ? 1 : null
}

/** `order` with `id` moved one place by `delta`, or null at an end. */
export function movedOrder<T>(order: readonly T[], id: T, delta: -1 | 1): T[] | null {
  const from = order.indexOf(id)
  const to = from + delta
  if (from < 0 || to < 0 || to >= order.length) return null
  const next = [...order]
  next[from] = next[to]
  next[to] = id
  return next
}

/** `full` with the members of `group` laid out in `groupOrder` instead. */
export function withGroupOrder<T>(full: readonly T[], groupOrder: readonly T[]): T[] {
  const members = new Set(groupOrder)
  let i = 0
  return full.map((id) => (members.has(id) ? groupOrder[i++] : id))
}

/** Where a tab lands when a keyboard move runs past the edge of its group. */
export interface GroupCrossing<F> {
  /** The folder it joins, or null for the tabs outside folders. */
  folder: F | null
  /** Its slot among that group's tabs. */
  index: number
}

/**
 * `groups` are the open folders in strip order, then the tabs outside
 * folders. A tab moved past its group's end joins the start of the next
 * group, past its start the end of the one before; at the strip's ends it
 * stays put (null).
 */
export function crossedGroup<F>(
  groups: ReadonlyArray<{ folder: F | null; size: number }>,
  from: F | null,
  delta: -1 | 1,
): GroupCrossing<F> | null {
  const at = groups.findIndex((g) => g.folder === from)
  const target = at < 0 ? undefined : groups[at + delta]
  if (!target) return null
  return { folder: target.folder, index: delta === 1 ? 0 : target.size }
}

/** Spoken after a keyboard move. */
export function movedTo(position: number, count: number): string {
  return `Moved to ${position} of ${count}`
}

/** Spoken after a keyboard move into a folder, or out of one. */
export function movedAcross(into: string | null, outOf: string | null, position: number, count: number): string {
  const where = into !== null ? `Moved into ${into}` : `Moved out of ${outOf ?? 'the folder'}`
  return `${where}, ${position} of ${count}`
}

/**
 * Run `move` for an Alt+arrow press on the focused row itself. Returns
 * whether it was one.
 */
export function moveByKey(
  e: TabKey & { currentTarget: HTMLElement; target: EventTarget; preventDefault(): void },
  orientation: 'vertical' | 'horizontal',
  move: ((delta: -1 | 1) => void) | undefined,
): boolean {
  if (!move || e.target !== e.currentTarget) return false
  const delta = moveDelta(e, orientation)
  if (delta === null) return false
  e.preventDefault()
  move(delta)
  return true
}

/** Put focus back on `selector` once a reorder has re-rendered. */
export function refocusAfterMove(selector: string): void {
  requestAnimationFrame(() => document.querySelector<HTMLElement>(selector)?.focus())
}
