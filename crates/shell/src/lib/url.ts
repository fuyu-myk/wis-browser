export function normaliseUrl(input: string): string | null {
  const trimmed = input.trim()
  if (!trimmed) return null

  try {
    const u = new URL(trimmed)
    // Reject things like `mailto:` here.
    if (u.protocol === 'http:' || u.protocol === 'https:' || u.protocol === 'about:') {
      return u.toString()
    }
  } catch {
    /* fall through */
  }

  if (looksLikeHost(trimmed)) {
    return `https://${trimmed}`
  }

  return `https://duckduckgo.com/?q=${encodeURIComponent(trimmed)}`
}

const HOST_RE = /^[A-Za-z0-9.\-_]+(:\d+)?(\/.*)?$/

function looksLikeHost(input: string): boolean {
  if (input.includes(' ')) return false
  if (!input.includes('.')) return false
  return HOST_RE.test(input)
}

/** Truthy when a tab's URL should not cause a native webview to be created. */
export function isChromeUrl(url: string): boolean {
  return url.startsWith('about:')
}

/** Chrome page URLs treated as opaque sentinels by the rest of the app. */
export const NEW_TAB_URL = 'about:blank'
export const SETTINGS_URL = 'about:settings'

export function tabDisplayName(tab: { title: string; url: string }): string {
  if (tab.title && tab.title !== tab.url) {
    return shortTitle(tab.title, tab.url)
  }
  if (tab.url === SETTINGS_URL) return 'Settings'
  try {
    const u = new URL(tab.url)
    if (u.protocol === 'about:') {
      return 'Launchpad'
    }
    return u.host.replace(/^www\./, '')
  } catch {
    return tab.title || tab.url
  }
}

const TITLE_SEPARATOR_RE = /\s+[-|·—–»]\s+|:\s+/g

function shortTitle(title: string, url: string): string {
  const parts = title.split(TITLE_SEPARATOR_RE).map((p) => p.trim()).filter(Boolean)
  if (parts.length <= 1) return title.trim()

  const brand = brandTokenFor(url)
  if (brand) {
    const brandIndex = parts.findIndex((p) => matchesBrand(p, brand))
    if (brandIndex === 0) return parts[0]
    if (brandIndex > 0) {
      const remaining = parts.filter((_, i) => i !== brandIndex)
      return remaining.reduce((longest, p) => (p.length > longest.length ? p : longest))
    }
  }

  return parts.reduce((shortest, p) => (p.length < shortest.length ? p : shortest))
}

function matchesBrand(segment: string, brand: string): boolean {
  const normalized = segment.toLowerCase().replace(/[^a-z0-9]/g, '')
  return normalized === brand || normalized.startsWith(brand)
}

function brandTokenFor(url: string): string | null {
  try {
    const host = new URL(url).hostname.toLowerCase().replace(/^www\./, '')
    if (!host) return null
    const segments = host.split('.')
    if (segments.length === 0) return null
    // Second-level domain: youtube.com -> "youtube", en.wikipedia.org -> "wikipedia".
    return segments.length >= 2 ? segments[segments.length - 2] : segments[0]
  } catch {
    return null
  }
}
