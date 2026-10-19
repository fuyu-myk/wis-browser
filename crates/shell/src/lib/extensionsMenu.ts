export const EXT_MENU_OPEN = 'wis-extmenu-open'
export const EXT_MENU_READY = 'wis-extmenu-ready'
export const EXT_MENU_DONE = 'wis-extmenu-done'
/** The prompt's measured height, from the surface. */
export const EXT_PROMPT_SIZE = 'wis-extmenu-prompt-size'

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
const PADDING = 6
const BORDER = 1
const DIVIDER = 9
const GAP = 6
const MARGIN = 8

/** One row per extension (one for the empty note) with its site lines, then
 *  "Manage extensions". */
export function menuHeight(rows: number, siteLines = 0): number {
  return BORDER * 2 + PADDING * 2 + Math.max(rows, 1) * ROW + siteLines * SITE_LINE + DIVIDER + ROW
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
): Box {
  return below(button, MENU_WIDTH, menuHeight(rows, siteLines), viewport)
}

/** Where a request for more access opens, placed like the menu. */
export function promptBounds(button: Box, height: number, viewport: { width: number; height: number }): Box {
  return below(button, PROMPT_WIDTH, height, viewport)
}
