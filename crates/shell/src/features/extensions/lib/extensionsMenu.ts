export const EXT_MENU_OPEN = 'wis-extmenu-open'
export const EXT_MENU_READY = 'wis-extmenu-ready'
export const EXT_MENU_DONE = 'wis-extmenu-done'
/** A panel's measured height, from the surface. */
export const EXT_PROMPT_SIZE = 'wis-extmenu-prompt-size'
/** The declined store item, from the surface. */
export const EXT_STORE_DECLINED = 'wis-extmenu-store-declined'

export interface Box {
  x: number
  y: number
  width: number
  height: number
}

export const MENU_WIDTH = 264
export const PROMPT_WIDTH = 300
/** The prompt's height until the surface has measured it. */
export const PROMPT_HEIGHT = 150
const ROW = 34
/** The line under a row offering the active tab's site. */
export const SITE_LINE = 26
/** The offer to add a Chrome Web Store page's extension, with its divider. */
export const OFFER_BLOCK = 83
const PADDING = 6
const BORDER = 1
const DIVIDER = 9
const GAP = 6
const MARGIN = 8

/** The store offer when there is one, one row per extension (one for the
 *  empty note) with its site lines, then "Manage extensions". */
export function menuHeight(rows: number, siteLines = 0, offer = false): number {
  return (
    BORDER * 2 +
    PADDING * 2 +
    (offer ? OFFER_BLOCK : 0) +
    Math.max(rows, 1) * ROW +
    siteLines * SITE_LINE +
    DIVIDER +
    ROW
  )
}

/** A Chrome Web Store item page, where the menu offers to add its extension. */
export function isStoreItemPage(url: string): boolean {
  try {
    const { protocol, hostname, pathname } = new URL(url)
    if (protocol !== 'https:') return false
    if (hostname === 'chromewebstore.google.com') return pathname.startsWith('/detail/')
    return hostname === 'chrome.google.com' && pathname.startsWith('/webstore/detail/')
  } catch {
    return false
  }
}

function below(button: Box, width: number, height: number, viewport: { width: number; height: number }): Box {
  const x = Math.min(Math.max(MARGIN, button.x + button.width - width), viewport.width - width - MARGIN)
  const under = button.y + button.height + GAP
  const y = under + height > viewport.height - MARGIN ? Math.max(MARGIN, button.y - GAP - height) : under
  return { x, y, width, height }
}

/** Below `button` with the right edges lined up, above it when there is no
 *  room below, and always inside the window. */
export function menuBounds(
  button: Box,
  rows: number,
  viewport: { width: number; height: number },
  siteLines = 0,
  offer = false,
): Box {
  return below(button, MENU_WIDTH, menuHeight(rows, siteLines, offer), viewport)
}

/** Where a request for more access opens, placed like the menu. */
export function promptBounds(button: Box, height: number, viewport: { width: number; height: number }): Box {
  return below(button, PROMPT_WIDTH, height, viewport)
}
