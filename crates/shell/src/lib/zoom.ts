/**
 * The chrome's read-only view of page zoom, mirroring
 * `wis_core::zoom::zoom_percent_for_host`. Both writers (the zoom command and
 * the settings boundary) snap to the ladder before storing.
 */

/** The host of a page URL, or an empty string when it has none. */
export function hostOfUrl(url: string): string {
  try {
    return new URL(url).hostname
  } catch {
    return ''
  }
}

/** The zoom for `host`: its own percentage when set, else the default. */
export function zoomPercentForHost(
  host: string,
  overrides: Record<string, number>,
  defaultPercent: number,
): number {
  const key = host.trim().toLowerCase().replace(/^www\./, '')
  return overrides[key] ?? defaultPercent
}
