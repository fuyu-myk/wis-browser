/**
 * Values in a canvas page obtained from Rust evaluating `window.<key> = <json>`:
 * the browser's theme accent (`__wisSwipeAccent`, set in every tab after each
 * load and whenever the theme changes, `swipe_nav.rs`) and the canvas theme
 * (`__wisCanvasTheme`, set at start and on change, `canvas.rs`).
 */

export function followPushed(
  host: object,
  key: string,
  apply: (value: string) => void,
  accept: (value: string) => boolean,
): void {
  let current: string | undefined
  const take = (value: unknown) => {
    if (typeof value !== 'string' || !accept(value)) return
    current = value
    apply(value)
  }
  take((host as Record<string, unknown>)[key])
  Object.defineProperty(host, key, {
    get: () => current,
    set: take,
    configurable: false,
  })
}
