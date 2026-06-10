import { useEffect, useState } from 'react'

import { commands } from '@/bindings/commands'
import { cx } from '@/lib/cx'

// Process-wide cache: a widget's favicon is the same across every sidebar.
const dataUrlCache = new Map<string, string | null>()
const inflight = new Map<string, Promise<string | null>>()

const FALLBACK_PALETTES = ['fav-pri', 'fav-mint', 'fav-rose', 'fav-sun', 'fav-lila', 'fav-azur']

/**
 * Resolve a site favicon to a `data:` URL, deduping concurrent lookups and
 * caching the result for the session. `prefetchFavicon` ensures the icon is in
 * the on-disk cache (fetching it once, network only on a cold cache) and
 * returns its hash; `getFaviconDataUrl` turns that into a `data:` URL.
 */
function resolve(url: string): Promise<string | null> {
  const cached = dataUrlCache.get(url)
  if (cached !== undefined) return Promise.resolve(cached)
  let pending = inflight.get(url)
  if (!pending) {
    pending = (async () => {
      const hash = await commands.prefetchFavicon(url).catch(() => null)
      return hash ? await commands.getFaviconDataUrl(hash).catch(() => null) : null
    })().then((dataUrl) => {
      inflight.delete(url)
      dataUrlCache.set(url, dataUrl ?? null)
      return dataUrl ?? null
    })
    inflight.set(url, pending)
  }
  return pending
}

/**
 * Warm the favicon cache for a set of widget URLs (call on startup). Each
 * resolves through the shared cache, so the dock and settings render their
 * icons immediately instead of flashing the letter-tile fallback.
 */
export function prefetchWidgetFavicons(urls: readonly string[]): void {
  for (const url of urls) void resolve(url)
}

/** A widget's site favicon, with a coloured-letter fallback while it loads. */
export function WidgetFavicon({ url, label }: { url: string; label: string }) {
  const [src, setSrc] = useState<string | null>(() => dataUrlCache.get(url) ?? null)

  useEffect(() => {
    let alive = true
    void resolve(url).then((dataUrl) => {
      if (alive) setSrc(dataUrl)
    })
    return () => {
      alive = false
    }
  }, [url])

  if (src) return <img className="kb-widget-fav-img" src={src} alt="" draggable={false} />

  const ch = (label.trim()[0] ?? '?').toUpperCase()
  const palette = FALLBACK_PALETTES[ch.charCodeAt(0) % FALLBACK_PALETTES.length]
  return <span className={cx('kb-widget-fav-tile', palette)}>{ch}</span>
}
