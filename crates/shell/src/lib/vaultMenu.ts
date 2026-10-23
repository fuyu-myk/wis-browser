/**
 * Event contract for the URL-bar "vault key" dropdown (overlay-vaultmenu),
 * which hosts both the "Save password?" prompt and the saved-account picker.
 *
 * The key button in the address bar (chrome) owns positioning + open/close; the
 * overlay surface is mostly self-contained (reads the active tab from browser
 * state, fetches candidates/pending) and reports its measured height + a
 * close request back.
 */

import type { Address, TabId } from '@/bindings/types'

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

/** native → all: a page submitted an address worth offering to save. */
export const ADDRESS_CAPTURE_EVENT = 'wis-address-capture'

/** A submitted address waiting for "Save address?"; a save names its serial. */
export interface PendingAddress {
  serial: number
  address: Address
  /** One was dismissed in this tab lately: wait to be opened. */
  quiet: boolean
}

export interface AddressCaptureEvent extends PendingAddress {
  tab: TabId
}

/** Saved-logins card. */
export const VAULTMENU_FILL_EVENT = 'wis-vaultmenu-fill'

/** Where to place the card within the full-window overlay (window coords). */
export interface VaultMenuAnchor {
  x: number
  y: number
  width: number
  /** Unlock the vault for `tab`, then fill the saved login, if one is named. */
  fill?: { tab: TabId; credentialId?: number }
}

/** A card at a field asking the menu to unlock with the vault password, then
 *  fill the login it names, if any. */
export interface VaultMenuFill {
  window: string
  tab: TabId
  credentialId?: number
}
