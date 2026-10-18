export const EXT_MENU_OPEN = 'wis-extmenu-open'
export const EXT_MENU_READY = 'wis-extmenu-ready'
export const EXT_MENU_DONE = 'wis-extmenu-done'

export interface Box {
  x: number
  y: number
  width: number
  height: number
}

export const MENU_WIDTH = 264
const ROW = 34
const PADDING = 6
const BORDER = 1
const DIVIDER = 9
const GAP = 6
const MARGIN = 8

/** One row per extension (one for the empty note), then "Manage extensions". */
export function menuHeight(rows: number): number {
  return BORDER * 2 + PADDING * 2 + Math.max(rows, 1) * ROW + DIVIDER + ROW
}

/** Below `button` with the right edges lined up, above it when there is no
 *  room below, and always inside the window. */
export function menuBounds(button: Box, rows: number, viewport: { width: number; height: number }): Box {
  const height = menuHeight(rows)
  const x = Math.min(
    Math.max(MARGIN, button.x + button.width - MENU_WIDTH),
    viewport.width - MENU_WIDTH - MARGIN,
  )
  const below = button.y + button.height + GAP
  const y = below + height > viewport.height - MARGIN ? Math.max(MARGIN, button.y - GAP - height) : below
  return { x, y, width: MENU_WIDTH, height }
}
