/**
 * General-purpose transient toast, shown top-right over the page content.
 *
 * A toast must composite above the tab webviews, so it renders in a dedicated
 * `overlay-toast` webview (the same reason the screenshot toast does) rather
 * than the chrome tree, which the page occludes. A caller anywhere in the
 * chrome dispatches `TOAST_REQUEST_EVENT`; `useToastOverlay` shows the overlay
 * and forwards the message to `ToastSurface` over `TOAST_SHOW_EVENT`.
 */

import { emit } from '@tauri-apps/api/event'

/** DOM event a chrome caller dispatches to raise a toast. */
export const TOAST_REQUEST_EVENT = 'wis-toast-request'
/** Tauri event: chrome -> surface, carries the message to display. */
export const TOAST_SHOW_EVENT = 'wis-toast-show'
/** Tauri event: another webview -> a window's chrome, asking for a toast. */
export const TOAST_REMOTE_EVENT = 'wis-toast-remote'
/** Tauri event: surface -> chrome, the toast has dismissed. */
export const TOAST_CLOSED_EVENT = 'wis-toast-closed'
/** Tauri event: surface -> chrome, the surface has mounted, so a "show" that
 *  raced the lazily-created webview can be replayed. */
export const TOAST_READY_EVENT = 'wis-toast-ready'

export interface ToastRequest {
  message: string
}

/** A toast asked for from outside the chrome, addressed to one window. */
export interface RemoteToastRequest {
  window: string
  message: string
}

/** Raise a transient toast from anywhere in the chrome webview. */
export function showToast(message: string): void {
  window.dispatchEvent(
    new CustomEvent<ToastRequest>(TOAST_REQUEST_EVENT, { detail: { message } }),
  )
}

/**
 * Raise a toast from a webview that is not the chrome, such as an overlay
 * surface. `showToast` cannot reach it: the DOM event it dispatches stays
 * inside the webview that fired it.
 */
export function showRemoteToast(windowLabel: string, message: string): Promise<void> {
  return emit(TOAST_REMOTE_EVENT, { window: windowLabel, message }).catch(() => undefined)
}
