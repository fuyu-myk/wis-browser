/**
 * Where the generated-password offer sits relative to the field it is for.
 *
 * The page reports the field in its own CSS pixels. Those are not window
 * pixels once a site is zoomed, so the scale is measured, as the page menu
 * does: the pane's width over the width the page reported for its viewport.
 */

export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

export interface Size {
  width: number
  height: number
}

/** The field's rect in window coordinates. */
export function fieldInWindow(field: Rect, viewWidth: number, pane: Rect): Rect {
  const scale = viewWidth > 0 ? pane.width / viewWidth : 1
  return {
    x: pane.x + field.x * scale,
    y: pane.y + field.y * scale,
    width: field.width * scale,
    height: field.height * scale,
  }
}

/**
 * The card's rect: below the field, or above when only that fits, then held
 * inside the pane. When neither side fits it takes the roomier one. Null once
 * the field has left the pane, since a card pointing at nothing is worse than
 * none, and the key button stays as the way back.
 */
export function placeOffer(field: Rect, pane: Rect, card: Size, gap: number): Rect | null {
  const paneBottom = pane.y + pane.height
  if (field.y + field.height <= pane.y || field.y >= paneBottom) return null

  const below = field.y + field.height + gap
  const above = field.y - gap - card.height
  const fitsBelow = below + card.height <= paneBottom
  const fitsAbove = above >= pane.y
  const roomBelow = paneBottom - (field.y + field.height)
  const roomAbove = field.y - pane.y
  const y = fitsBelow ? below : fitsAbove ? above : roomBelow >= roomAbove ? below : above

  const x = Math.max(pane.x, Math.min(field.x, pane.x + pane.width - card.width))
  const top = Math.max(pane.y, Math.min(y, paneBottom - card.height))

  return { x, y: top, width: card.width, height: card.height }
}
