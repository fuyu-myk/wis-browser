/**
 * The browser's theme accent in a canvas page, obtained from Rust evaluating
 * `window.__wisSwipeAccent = "<colour>"` into every tab after each load and
 * whenever the theme changes (`swipe_nav.rs`).
 */

interface AccentHost {
  __wisSwipeAccent?: unknown
}

export function followThemeAccent(
  host: AccentHost,
  apply: (color: string) => void,
  isColor: (value: string) => boolean,
): void {
  let current: string | undefined
  const take = (value: unknown) => {
    if (typeof value !== 'string' || !isColor(value)) return
    current = value
    apply(value)
  }
  take(host.__wisSwipeAccent)
  Object.defineProperty(host, '__wisSwipeAccent', {
    get: () => current,
    set: take,
    configurable: false,
  })
}
