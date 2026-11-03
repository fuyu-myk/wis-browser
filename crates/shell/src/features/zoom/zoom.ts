/**
 * The chrome's read-only view of page zoom, mirroring
 * `wis_core::sites::zoom::zoom_percent_for_host`. Both writers (the zoom command and
 * the settings boundary) snap to the ladder before storing, but a settings file is
 * loaded as written, so reads snap too.
 */

import { siteUrl } from '@/features/site/lib/host'

/** `wis_core::sites::zoom::ZOOM_LADDER`, the percentages zoom steps through. */
export const ZOOM_LADDER: readonly number[] = [
  20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200, 220,
  240, 260, 280, 300, 400, 500,
]

/**
 * `wis_core::sites::zoom::sane_zoom_percent`: the rung nearest `percent`, ties
 * to the smaller.
 */
export function saneZoomPercent(percent: number): number {
  return ZOOM_LADDER.reduce((best, rung) =>
    Math.abs(rung - percent) < Math.abs(best - percent) ? rung : best,
  )
}

/** The host of a page URL's site (`siteUrl`), or an empty string when it has none. */
export function hostOfUrl(url: string): string {
  return siteUrl(url)?.hostname ?? ''
}

/** The zoom for `host`: its own percentage when set, else the default. */
export function zoomPercentForHost(
  host: string,
  overrides: Record<string, number>,
  defaultPercent: number,
): number {
  const key = host.trim().toLowerCase().replace(/^www\./, '')
  return saneZoomPercent(
    Object.prototype.hasOwnProperty.call(overrides, key) ? overrides[key] : defaultPercent,
  )
}
