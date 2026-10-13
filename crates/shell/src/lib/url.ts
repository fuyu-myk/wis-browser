import type { CustomSearchEngine, SearchEngine, UserSettings } from '@/bindings/types'

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

/** The settings slice search resolution needs. */
export type SearchSettings = Pick<
  UserSettings,
  'search_engine' | 'custom_search_engines' | 'builtin_search_keywords'
>

interface ResolvedEngine {
  label: string
  search: (query: string) => string
}

function customEngine(engine: CustomSearchEngine): ResolvedEngine {
  return {
    label: engine.name,
    search: (q) => engine.template.split('{searchTerms}').join(encodeURIComponent(q)),
  }
}

/**
 * The engine non-URL input searches with: the custom engine marked default
 * when one exists, else the built-in choice.
 */
export function activeEngine(settings?: SearchSettings): ResolvedEngine {
  const custom = settings?.custom_search_engines.find((e) => e.is_default)
  if (custom) return customEngine(custom)
  const engine = settings?.search_engine ?? DEFAULT_ENGINE
  return SEARCH_ENGINES[engine] ?? SEARCH_ENGINES[DEFAULT_ENGINE]
}

/** A resolved address-bar input. */
export interface ResolvedInput {
  url: string
  /** True when the input became an engine query. */
  isSearch: boolean
  /** Engine label when `isSearch`; null otherwise. */
  engineLabel: string | null
}

/**
 * Turn raw address-bar input into a navigable URL. Keyword triggers
 * (`kw query`; custom engines first, then built-in keyword assignments) are
 * checked before the host heuristic so a keyword can never be shadowed; then
 * full URLs, hosts, and finally a search with the active engine. A bare
 * keyword with no query falls through to normal handling.
 */
export function resolveInput(raw: string, settings?: SearchSettings): ResolvedInput | null {
  const trimmed = raw.trim()
  if (!trimmed) return null

  const space = trimmed.search(/\s/)
  if (space > 0) {
    const keyword = trimmed.slice(0, space).toLowerCase()
    const rest = trimmed.slice(space + 1).trim()
    if (rest) {
      const custom = settings?.custom_search_engines.find((e) => e.keyword === keyword)
      if (custom) {
        const engine = customEngine(custom)
        return { url: engine.search(rest), isSearch: true, engineLabel: engine.label }
      }
      const builtins = settings?.builtin_search_keywords ?? {}
      const builtin = (Object.keys(builtins) as SearchEngine[]).find(
        (e) => builtins[e] === keyword && SEARCH_ENGINES[e],
      )
      if (builtin) {
        const engine = SEARCH_ENGINES[builtin]
        return { url: engine.search(rest), isSearch: true, engineLabel: engine.label }
      }
    }
  }

  try {
    const u = new URL(trimmed)
    // Reject things like `mailto:` here.
    if (['http:', 'https:', 'about:', 'file:'].includes(u.protocol)) {
      return { url: u.toString(), isSearch: false, engineLabel: null }
    }
  } catch {
    /* fall through */
  }

  if (looksLikeHost(trimmed)) {
    return { url: `${schemeFor(trimmed)}${trimmed}`, isSearch: false, engineLabel: null }
  }

  const engine = activeEngine(settings)
  return { url: engine.search(trimmed), isSearch: true, engineLabel: engine.label }
}

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
    return `${schemeFor(trimmed)}${trimmed}`
  }

  return (SEARCH_ENGINES[engine] ?? SEARCH_ENGINES[DEFAULT_ENGINE]).search(trimmed)
}

const HOST_RE = /^[A-Za-z0-9.\-_]+(:\d+)?(\/.*)?$/
const LOCAL_RE = /^(localhost|\[[0-9A-Fa-f:.]+\])(:\d+)?(\/.*)?$/i
const PORTED_RE = /^[A-Za-z][A-Za-z0-9-]*:\d+(\/.*)?$/
const LOOPBACK_RE = /^((?:[a-z0-9-]+\.)*localhost|127\.\d+\.\d+\.\d+|\[::1\])(:|\/|$)/i

function looksLikeHost(input: string): boolean {
  if (input.includes(' ')) return false
  if (LOCAL_RE.test(input) || PORTED_RE.test(input)) return true
  if (!input.includes('.')) return false
  return HOST_RE.test(input)
}

function schemeFor(host: string): string {
  return LOOPBACK_RE.test(host) ? 'http://' : 'https://'
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

/**
 * Whether `href` is a plain web link, safe to open in a tab from an untrusted
 * source (model output, page content).
 * 
 * Everything else is refused. Compares `URL.protocol`, never a string prefix.
 */
export function isWebUrl(href: string): boolean {
  try {
    const parsed = new URL(href)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  } catch {
    return false
  }
}

/** Truthy when a tab's URL should not cause a native webview to be created. */
export function isChromeUrl(url: string): boolean {
  return url.startsWith('about:')
}

/** Chrome page URLs treated as opaque sentinels by the rest of the app. */
export const NEW_TAB_URL = 'about:blank'

/**
 * The URL a freshly opened tab should load, honouring the new-tab preference:
 * the homepage when chosen and set, otherwise the launchpad. Falls back to the
 * launchpad if settings haven't loaded yet.
 */
export function resolveNewTabUrl(settings: UserSettings | undefined): string {
  if (settings?.new_tab_target === 'homepage' && settings.homepage) {
    return settings.homepage
  }
  return NEW_TAB_URL
}
export const SETTINGS_URL = 'about:settings'
export const HISTORY_URL = 'about:history'
export const DOWNLOADS_URL = 'about:downloads'
export const USAGE_URL = 'about:usage'
export const CANVAS_URL = 'wis-canvas://localhost/'

export function isCanvasUrl(url: string): boolean {
  return url.startsWith('wis-canvas:')
}
export const SOURCE_URL = 'about:source'

/** A source tab's URL: the page it shows, and the tab captured from. */
export function sourceUrlFor(target: string, tab: string): string {
  return `${SOURCE_URL}?url=${encodeURIComponent(target)}&tab=${encodeURIComponent(tab)}`
}

/**
 * What a source tab shows, or null when `url` is not one. Matched by prefix.
 */
export function sourceTargetOf(url: string): { url: string; tab: string } | null {
  if (url !== SOURCE_URL && !url.startsWith(`${SOURCE_URL}?`)) return null
  try {
    const params = new URL(url).searchParams
    return { url: params.get('url') ?? '', tab: params.get('tab') ?? '' }
  } catch {
    return null
  }
}

export function tabDisplayName(tab: { title: string; url: string }): string {
  if (sourceTargetOf(tab.url) !== null) return 'Page source'
  if (tab.title && tab.title !== tab.url) {
    return shortTitle(tab.title, tab.url)
  }
  if (tab.url === SETTINGS_URL) return 'Settings'
  if (tab.url === HISTORY_URL) return 'History'
  if (tab.url === DOWNLOADS_URL) return 'Downloads'
  if (tab.url === USAGE_URL) return 'Usage'
  if (isCanvasUrl(tab.url)) return 'Canvas'
  try {
    const u = new URL(tab.url)
    if (u.protocol === 'about:') {
      return 'Launchpad'
    }
    if (u.protocol === 'file:') {
      return decodeURIComponent(u.pathname.split('/').pop() ?? '') || tab.url
    }
    return u.host.replace(/^www\./, '')
  } catch {
    return tab.title || tab.url
  }
}

// Site separators only ("Page - Site", "Page | Site").
const TITLE_SEPARATOR_RE = /\s+[-|·—–»]\s+/g

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

  return parts[0]
}

function matchesBrand(segment: string, brand: string): boolean {
  const fold = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, '')
  const normalized = fold(segment)
  const folded = fold(brand)
  return normalized === folded || normalized.startsWith(folded)
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
