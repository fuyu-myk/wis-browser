/**
 * Animated tab close, usable from any chrome surface: broadcasts the closing
 * id (Tauri event, so overlay webviews can initiate too) and issues the close
 * command immediately (optimistic: the webview tears down and the new active
 * tab takes over on the spot). The tab strips keep rendering a ghost row for
 * the closed tab until its exit animation has played.
 */

import { emit, listen } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { TabId } from '@/bindings/types'

export const TAB_CLOSING_EVENT = 'wis-tab-closing'
export const TAB_EXIT_MS = 180

export function closeTabAnimated(id: TabId): void {
  void emit(TAB_CLOSING_EVENT, id).catch(() => undefined)
  void commands.closeTab(id).catch(() => undefined)
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
