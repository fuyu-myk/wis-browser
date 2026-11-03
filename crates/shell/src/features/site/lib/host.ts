/** Reduce a pasted URL or `host/path` to a `www.`-stripped, lowercased host. */
export function normalizeHostInput(raw: string): string {
  const s = raw.trim().toLowerCase()
  if (s === '') return ''
  // Parsing drops a scheme, path and port alike.
  try {
    const hasScheme = /^[a-z][a-z0-9+.-]*:\/\//.test(s)
    const host = new URL(hasScheme ? s : `http://${s}`).hostname
    return host.replace(/^www\./, '')
  } catch {
    return ''
  }
}

/**
 * The URL whose site a page belongs to, as `wis_core::sites::domain::site_url`
 * decides: `url` itself, or for a `blob:` URL one on the http(s) origin that
 * made it, where only the scheme, host and port mean anything.
 */
export function siteUrl(url: string): URL | null {
  try {
    const parsed = new URL(url)
    if (parsed.protocol !== 'blob:') return parsed
    const inner = new URL(parsed.pathname)
    return inner.protocol === 'http:' || inner.protocol === 'https:' ? inner : null
  } catch {
    return null
  }
}
