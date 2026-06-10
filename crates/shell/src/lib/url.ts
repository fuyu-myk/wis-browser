import type { SearchEngine } from '@/bindings/types'

/** Per-engine metadata: display label + query-URL builder. */
export const SEARCH_ENGINES: Record<
  SearchEngine,
  { label: string; search: (query: string) => string }
> = {
  duckduckgo: {
    label: 'DuckDuckGo',
    search: (q) => `https://duckduckgo.com/?q=${encodeURIComponent(q)}`,
  },
  google: {
    label: 'Google',
    search: (q) => `https://www.google.com/search?q=${encodeURIComponent(q)}`,
  },
  bing: {
    label: 'Bing',
    search: (q) => `https://www.bing.com/search?q=${encodeURIComponent(q)}`,
  },
}

const DEFAULT_ENGINE: SearchEngine = 'duckduckgo'

export function normaliseUrl(input: string, engine: SearchEngine = DEFAULT_ENGINE): string | null {
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

  return (SEARCH_ENGINES[engine] ?? SEARCH_ENGINES[DEFAULT_ENGINE]).search(trimmed)
}

const HOST_RE = /^[A-Za-z0-9.\-_]+(:\d+)?(\/.*)?$/

function looksLikeHost(input: string): boolean {
  if (input.includes(' ')) return false
  if (!input.includes('.')) return false
  return HOST_RE.test(input)
}

/**
 * True when `normaliseUrl` would treat `input` as a search query (rather than
 * a navigable URL/host). Used to decide whether to remember it as a search.
 */
export function isSearchQuery(input: string): boolean {
  const trimmed = input.trim()
  if (!trimmed) return false
  try {
    const u = new URL(trimmed)
    if (u.protocol === 'http:' || u.protocol === 'https:' || u.protocol === 'about:') {
      return false
    }
  } catch {
    /* not a URL */
  }
  return !looksLikeHost(trimmed)
}

/** Truthy when a tab's URL should not cause a native webview to be created. */
export function isChromeUrl(url: string): boolean {
  return url.startsWith('about:')
}

/** Chrome page URLs treated as opaque sentinels by the rest of the app. */
export const NEW_TAB_URL = 'about:blank'
export const SETTINGS_URL = 'about:settings'
export const HISTORY_URL = 'about:history'
export const DOWNLOADS_URL = 'about:downloads'

export function tabDisplayName(tab: { title: string; url: string }): string {
  if (tab.title && tab.title !== tab.url) {
    return shortTitle(tab.title, tab.url)
  }
  if (tab.url === SETTINGS_URL) return 'Settings'
  if (tab.url === HISTORY_URL) return 'History'
  if (tab.url === DOWNLOADS_URL) return 'Downloads'
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
