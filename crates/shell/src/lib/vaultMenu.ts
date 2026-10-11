/**
 * Event contract for the URL-bar "vault key" dropdown (overlay-vaultmenu),
 * which hosts both the "Save password?" prompt and the saved-account picker.
 *
 * The key button in the address bar (chrome) owns positioning + open/close; the
 * overlay surface is mostly self-contained (reads the active tab from browser
 * state, fetches candidates/pending) and reports its measured height + a
 * close request back.
 */

import type { TabId } from '@/bindings/types'

/** chrome → surface: (re)load + render, anchoring the card at this rect. */
export const VAULTMENU_OPEN_EVENT = 'wis-vaultmenu-open'
/** surface → chrome: an action / outside-click finished; hide the overlay. */
export const VAULTMENU_CLOSE_EVENT = 'wis-vaultmenu-close'
/** native → all: a login was captured in a tab (carries a `CaptureEvent`). */
export const CAPTURE_LOGIN_EVENT = 'wis-capture-login'

export interface CaptureInfo {
  host: string
  username: string
  /** Wis filled this password, so nothing but this prompt holds it. */
  generated: boolean
  /** Saving replaces a password already stored under this username. */
  replaces: boolean
}

/** A captured login, with the tab it was captured in. */
export interface CaptureEvent extends CaptureInfo {
  tab: TabId
}

/** Where to place the card within the full-window overlay (window coords). */
export interface VaultMenuAnchor {
  x: number
  y: number
  width: number
}
