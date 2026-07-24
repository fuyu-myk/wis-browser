/**
 * Animated tab close, usable from any chrome surface: broadcasts the closing
 * id and issues the close command immediately (optimistic: the webview tears
 * down and the new active tab takes over on the spot). The tab strips keep
 * rendering a ghost row for the closed tab until its exit animation has played.
 *
 * The broadcast is **two-channel**: a synchronous in-document DOM event so
 * same-webview surfaces (the strips, key actions, task manager) begin the exit
 * animation + reflow on the very tick of the gesture, plus a Tauri event so any
 * other webview can initiate or observe a close too.
 */

import { emit, listen } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { TabId } from '@/bindings/types'
import { closeReturnTarget, noteTabClosed } from '@/lib/tabMru'

export const TAB_CLOSING_EVENT = 'wis-tab-closing'
export const TAB_EXIT_MS = 180

export function closeTabAnimated(id: TabId): void {
  window.dispatchEvent(new CustomEvent(TAB_CLOSING_EVENT, { detail: id }))
  void emit(TAB_CLOSING_EVENT, id).catch(() => undefined)
  const returnTo = closeReturnTarget(id)
  noteTabClosed(id)
  void commands.closeTab(id, returnTo).catch(() => undefined)
}

/**
 * Subscribe to closing-tab broadcasts; returns a cleanup function. Fires on the
 * synchronous DOM event (same webview) and the Tauri event (any webview); the
 * callback must be idempotent since a same-webview close delivers both.
 */
export function onTabClosing(cb: (id: TabId) => void): () => void {
  const onDom = (e: Event) => {
    const id = (e as CustomEvent<TabId>).detail
    if (id) cb(id)
  }
  window.addEventListener(TAB_CLOSING_EVENT, onDom)

  let unlisten: (() => void) | null = null
  let cancelled = false
  void listen<TabId>(TAB_CLOSING_EVENT, (e) => cb(e.payload)).then((fn) => {
    if (cancelled) fn()
    else unlisten = fn
  })
  return () => {
    cancelled = true
    window.removeEventListener(TAB_CLOSING_EVENT, onDom)
    unlisten?.()
  }
}
