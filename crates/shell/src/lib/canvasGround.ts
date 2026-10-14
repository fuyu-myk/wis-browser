/**
 * A canvas tab at the bottom of the window reaches past its pane, over the
 * content border and gutter, to the window's edge; it paints its own panel
 * inset by that gap so its prompt bar can stand on the edge. Only a gap of
 * plain gutter is taken, never chrome that lives below the content.
 */

interface Rect {
  x: number
  y: number
  width: number
  height: number
}

const MAX_GROUND = 24

export function canvasGround(input: {
  rect: Rect
  hostBottom: number
  windowHeight: number
  canvas: boolean
}): number {
  const { rect, hostBottom, windowHeight, canvas } = input
  const bottom = rect.y + rect.height
  if (!canvas || bottom < hostBottom - 1) return 0
  const gap = Math.round(windowHeight - bottom)
  return gap > 0 && gap <= MAX_GROUND ? gap : 0
}

export interface Ground {
  bottom: number
  radius: number
}

const MAX_RADIUS = 40
const sane = (n: unknown, max: number): n is number => typeof n === 'number' && Number.isFinite(n) && n >= 0 && n <= max

/** A ground pushed as `"bottom,radius"` or returned as an object. */
export function readGround(value: unknown): Ground | null {
  let bottom: unknown
  let radius: unknown
  if (typeof value === 'string') {
    const match = /^(\d+(?:\.\d+)?),(\d+(?:\.\d+)?)$/.exec(value)
    if (!match) return null
    bottom = Number(match[1])
    radius = Number(match[2])
  } else if (typeof value === 'object' && value !== null) {
    ;({ bottom, radius } = value as Record<string, unknown>)
  }
  return sane(bottom, MAX_GROUND) && sane(radius, MAX_RADIUS) ? { bottom, radius } : null
}
