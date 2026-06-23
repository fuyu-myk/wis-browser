/**
 * DOM event the chrome dispatches after a successful screenshot capture (the
 * image is already on the clipboard). The screenshot-toast hook listens for it
 * and shows the top-right "Copied" toast with a Save action.
 */
export const SCREENSHOT_CAPTURED_EVENT = 'wis-screenshot-captured'

/** Tauri event: the chrome tells the toast surface to appear. */
export const SCREENSHOT_TOAST_SHOWN_EVENT = 'wis-screenshot-toast-shown'

/** Tauri event: the toast surface tells the chrome it has dismissed. */
export const SCREENSHOT_TOAST_CLOSED_EVENT = 'wis-screenshot-toast-closed'

/** Tauri event: the toast surface announces it has mounted, so the chrome can
 *  replay a pending "shown" that raced the lazily-created overlay webview. */
export const SCREENSHOT_TOAST_READY_EVENT = 'wis-screenshot-toast-ready'

/** DOM event: the chrome opens the window-region selection overlay. Detail
 *  carries the active `tabId` (the backend ignores it for a window crop, but
 *  the command signature needs one). */
export const SCREENSHOT_REGION_OPEN_EVENT = 'wis-screenshot-region-open'

/** Tauri event: the chrome tells the region surface to begin a selection. */
export const SCREENSHOT_REGION_BEGIN_EVENT = 'wis-screenshot-region-begin'

/** Tauri event: the region surface announces it has mounted (replay handshake). */
export const SCREENSHOT_REGION_READY_EVENT = 'wis-screenshot-region-ready'

/** Tauri event: the region surface reports the chosen rect (or `null` cancel). */
export const SCREENSHOT_REGION_RESULT_EVENT = 'wis-screenshot-region-result'

/** A selection rectangle in the window content area (top-left origin, CSS px). */
export interface RegionRect {
  x: number
  y: number
  width: number
  height: number
}
