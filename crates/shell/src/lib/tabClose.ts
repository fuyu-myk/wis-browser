/**
 * Animated tab close, usable from any chrome surface: broadcasts the closing
 * id (Tauri event, so overlay webviews can initiate too), lets every tab list
 * play its exit, then issues the close command. The backend's
 * `browser-state-changed` refresh removes the row after the animation.
 */

import { emit, listen } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { TabId } from '@/bindings/types'

export const TAB_CLOSING_EVENT = 'wis-tab-closing'
export const TAB_EXIT_MS = 180

export function closeTabAnimated(id: TabId): void {
  void emit(TAB_CLOSING_EVENT, id).catch(() => undefined)
  window.setTimeout(() => {
    void commands.closeTab(id).catch(() => undefined)
  }, TAB_EXIT_MS)
}

/** Subscribe to closing-tab broadcasts; returns a cleanup function. */
export function onTabClosing(cb: (id: TabId) => void): () => void {
  let unlisten: (() => void) | null = null
  let cancelled = false
  void listen<TabId>(TAB_CLOSING_EVENT, (e) => cb(e.payload)).then((fn) => {
    if (cancelled) fn()
    else unlisten = fn
  })
  return () => {
    cancelled = true
    unlisten?.()
  }
}
