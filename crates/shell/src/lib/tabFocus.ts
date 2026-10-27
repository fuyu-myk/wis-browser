/**
 * Tab and Shift+Tab reach every control in Wis's own UI. WebKit on macOS
 * stops only at text fields unless the system "Keyboard navigation" setting
 * is on, so the chrome and overlay webviews move focus themselves. Web pages
 * keep the engine's default.
 */

const TABBABLE = [
  'a[href]',
  'button',
  'input:not([type="hidden"])',
  'select',
  'textarea',
  'summary',
  '[tabindex]',
  '[contenteditable="true"]',
].join(',')

/** The stop after `current` among `count`, wrapping round; -1 when there is none. */
export function nextStop(count: number, current: number, backwards: boolean): number {
  if (count === 0) return -1
  if (current < 0) return backwards ? count - 1 : 0
  return (current + (backwards ? count - 1 : 1)) % count
}

function visible(el: HTMLElement): boolean {
  return el.checkVisibility
    ? el.checkVisibility({ visibilityProperty: true, opacityProperty: true })
    : el.getClientRects().length > 0
}

function reachable(el: HTMLElement): boolean {
  return (
    el.tabIndex >= 0 &&
    !el.matches(':disabled') &&
    !el.closest('[inert], [aria-hidden="true"], .is-closing') &&
    visible(el)
  )
}

/** An open modal keeps Tab inside it. */
function scope(): ParentNode {
  const modals = [...document.querySelectorAll<HTMLElement>('[aria-modal="true"]')].filter(visible)
  return modals[modals.length - 1] ?? document
}

function stops(root: ParentNode): HTMLElement[] {
  return [...root.querySelectorAll<HTMLElement>(TABBABLE)].filter(reachable)
}

/** Where focus sits among the stops, counting a non-stop as the gap before the next one. */
function position(list: HTMLElement[], active: Element | null, backwards: boolean): number {
  if (!active || active === document.body) return -1
  const at = list.indexOf(active as HTMLElement)
  if (at >= 0) return at
  const after = list.findIndex(
    (el) => active.compareDocumentPosition(el) & Node.DOCUMENT_POSITION_FOLLOWING,
  )
  if (after < 0) return backwards ? list.length : list.length - 1
  return backwards ? after : after - 1
}

/**
 * Runs after React's handlers, so one that uses Tab calls preventDefault. A
 * plain listener that handles Tab itself marks its subtree `data-tab-native`.
 */
function onKeyDown(e: KeyboardEvent): void {
  if (e.key !== 'Tab' || e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return
  if (e.target instanceof Element && e.target.closest('[data-tab-native]')) return
  const list = stops(scope())
  const current = position(list, document.activeElement, e.shiftKey)
  const next = list[nextStop(list.length, current, e.shiftKey)]
  if (!next) return
  e.preventDefault()
  next.focus()
}

export function installTabFocus(): void {
  window.addEventListener('keydown', onKeyDown)
}
