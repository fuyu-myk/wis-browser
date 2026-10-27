/** The strip at the top of the Settings scroll area that titles pin to. */
export interface PinStrip {
  /** Where a pinned title's top sits. */
  line: number
  /** Content fades out above this. */
  band: number
  /** Space kept between a pinned title and the one pushing it. */
  gap: number
}

export const PIN_STRIP: PinStrip = { line: 12, band: 54, gap: 8 }

export interface PinnedCopy {
  index: number
  top: number
}

/**
 * The section titles to draw over the strip, by index into `tops` (the
 * titles' offsets from the scroll area's top, in order) with where each
 * goes.
 */
export function pinLayout(
  tops: readonly number[],
  heights: readonly number[],
  strip: PinStrip,
): PinnedCopy[] {
  let pinned = -1
  tops.forEach((top, i) => {
    if (top <= strip.line) pinned = i
  })
  const copies: PinnedCopy[] = []
  const next = pinned + 1
  if (pinned >= 0) {
    const pushFrom = strip.line + heights[pinned] + strip.gap
    const push = next < tops.length && tops[next] < pushFrom ? tops[next] - pushFrom : 0
    copies.push({ index: pinned, top: strip.line + push })
  }
  if (next < tops.length && tops[next] < strip.band) copies.push({ index: next, top: tops[next] })
  return copies
}
