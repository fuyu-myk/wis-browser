/**
 * The contract between the page-menu opener hook and its overlay surface.
 *
 * Event names and the payload live here rather than as string literals on both
 * sides, so the two cannot drift (`lib/toast.ts` is the same shape).
 */

import type { TabId, WorkspaceId } from '@/bindings/types'

/** Chrome to surface: open the menu on this target. */
export const PAGE_MENU_OPEN_EVENT = 'wis-pagemenu-open'
/** Surface to chrome: mounted, replay any open that raced webview creation. */
export const PAGE_MENU_READY_EVENT = 'wis-pagemenu-ready'
/** Surface to chrome: closed. */
export const PAGE_MENU_DONE_EVENT = 'wis-pagemenu-done'
/**
 * Surface to chrome: this open was accepted and is on screen.
 */
export const PAGE_MENU_ACK_EVENT = 'wis-pagemenu-ack'

/** What the page reported, as it reaches the chrome from `page_menu.rs`. */
export interface PageMenuReport {
  tabId: string
  pageUrl: string
  /** Pointer position in the page's own CSS pixels. */
  x: number
  y: number
  /** The page viewport in those same units. */
  viewWidth: number
  viewHeight: number
  linkHref: string | null
  linkText: string | null
  imageSrc: string | null
  mediaKind: string | null
  mediaSrc: string | null
  selection: string | null
}

/** Which surface the click landed on. */
export type PageMenuKind = 'page' | 'chrome'

export const MENU_URL_ATTR = 'data-menu-url'
export const MENU_LABEL_ATTR = 'data-menu-label'

/** What the surface needs to draw and act. */
export interface PageMenuTarget {
  kind: PageMenuKind
  /** Rises per open. */
  seq: number
  /** False when the click landed on a pane that was not the active one. */
  isActiveTab: boolean
  /** The window that opened it; events reach every window's surface. */
  window: string
  tabId: TabId
  workspaceId: WorkspaceId
  /** Pointer position in window-logical pixels. */
  x: number
  y: number
  /** The window's width. */
  winW: number
  /** The window's height. */
  winH: number
  /** The same point relative to the content host, which is the space Peek
   *  positions in. -1 when the host could not be measured. */
  peekX: number
  peekY: number
  linkHref: string | null
  linkText: string | null
  imageSrc: string | null
  /** `'video'` or `'audio'` when the click landed on a media element. */
  mediaKind: string | null
  /** That element's resolved source, which a `<source>` child may have chosen. */
  mediaSrc: string | null
  selection: string | null
  canGoBack: boolean
  canGoForward: boolean
  /** The link with its tracking parameters stripped, when any were found. */
  cleanLink: string | null
  /** Names that stripping would remove; empty means the link is already clean. */
  cleanRemoved: string[]
  /** A private workspace saves nothing, so some rows do not apply. */
  ephemeral: boolean
  /** Reader mode rewrote the document, so a capture would be Wis's markup. */
  reader: boolean
}
