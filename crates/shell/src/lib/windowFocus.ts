// Per-window focus tracking, shared by every cross-window event listener in
// this webview.
//
// App-wide events (`wis-keyaction` from the menu/accelerators, `wis-peek-open`
// from an Opt+click) are emitted with `app.emit`, which broadcasts to *every*
// window: Tauri listeners registered with the default `Any` target always
// match, so a backend `emit_filter` cannot scope them (see
// tauri event listener `match_any_or_filter`). The browser is one workspace per
// window, so a broadcast keyaction would create a tab / open a peek in every
// window at once. Each window therefore ignores these events unless it is the
// focused one.

import { getCurrentWindow } from '@tauri-apps/api/window'

// A window is focused the moment it opens; corrected immediately by isFocused()
// and kept live by onFocusChanged.
let focused = true

void getCurrentWindow()
  .isFocused()
  .then((f) => {
    focused = f || document.hasFocus()
  })
  .catch(() => undefined)

void getCurrentWindow()
  .onFocusChanged((e) => {
    focused = e.payload
  })
  .catch(() => undefined)

window.addEventListener('focus', () => {
  focused = true
})
window.addEventListener('pointerdown', () => {
  focused = true
}, { capture: true })

/**
 * Whether this webview's OS window currently has focus. Gate handlers for
 * app-wide broadcast events on this so only the window the user is actually in
 * reacts.
 */
export function isWindowFocused(): boolean {
  return focused
}
