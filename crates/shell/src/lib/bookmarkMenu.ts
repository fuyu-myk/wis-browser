/**
 * Event contract for the bookmarks popups.
 *
 * - The bookmarks glyph (sidebar/rail) dispatches `BOOKMARKS_OPEN_EVENT` (a DOM
 *   event) to toggle the full-window `overlay-bookmarks` popup (palette-style).
 * - The star folder-picker (`overlay-bmpicker`) is anchored under the URL-bar
 *   star (vault-menu style); the star emits `BMPICKER_OPEN_EVENT` (Tauri) with
 *   the just-added bookmark and an anchor rect.
 */

import type { WorkspaceId } from '@/bindings/types'

/** DOM event: glyph → chrome hook, toggles the bookmarks popup. */
export const BOOKMARKS_OPEN_EVENT = 'wis-bookmarks-open'
/** Tauri: surface → chrome hook, resets the toggle after close. */
export const BOOKMARKS_CLOSED_EVENT = 'wis-bookmarks-closed'

/** localStorage key for the last folder a bookmark was filed into. */
export const lastFolderKey = (ws: WorkspaceId) => `wis-bm-last-folder-${ws}`

/**
 * Auto-pin cap: newly starred bookmarks pin themselves only while fewer than
 * this many are already pinned (one row of quick-access bubbles). Beyond the cap
 * the user pins manually. Manual pins are not capped.
 */
export const PIN_CAP = 6

/** Tauri: star → picker surface, anchor + which bookmark to (re)file. */
export const BMPICKER_OPEN_EVENT = 'wis-bmpicker-open'
/** Tauri: picker surface → star, closed (so the star can resync). */
export const BMPICKER_CLOSE_EVENT = 'wis-bmpicker-close'

export interface BmPickerOpen {
  bookmarkId: number
  workspaceId: WorkspaceId
  currentFolderId: number | null
  /** Anchor rect of the star button (window coords). */
  x: number
  y: number
  width: number
}
