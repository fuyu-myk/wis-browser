import { useEffect, useState } from 'react'

import { commands } from '@/bindings/commands'
import { cx } from '@/lib/cx'

const FALLBACK = ['fav-pri', 'fav-mint', 'fav-rose', 'fav-sun', 'fav-lila', 'fav-azur']
const MISS_RETRY_MS = 5 * 60_000

const found = new Map<string, string>()
const missed = new Map<string, number>()
const pending = new Map<string, Promise<string | null>>()

async function lookup(site: string): Promise<string | null> {
  for (const origin of [`https://${site}`, `https://www.${site}`]) {
    const hash = await commands.faviconOriginHash(origin).catch(() => null)
    const dataUrl = hash ? await commands.getFaviconDataUrl(hash).catch(() => null) : null
    if (dataUrl) return dataUrl
  }
  return null
}

/** Shared across rows and remounts; a miss is asked again after a while. */
function faviconFor(site: string): Promise<string | null> {
  const hit = found.get(site)
  if (hit) return Promise.resolve(hit)
  const missedAt = missed.get(site)
  if (missedAt !== undefined && Date.now() - missedAt < MISS_RETRY_MS) return Promise.resolve(null)

  let request = pending.get(site)
  if (!request) {
    request = lookup(site).then((dataUrl) => {
      pending.delete(site)
      if (dataUrl) found.set(site, dataUrl)
      else missed.set(site, Date.now())
      return dataUrl
    })
    pending.set(site, request)
  }
  return request
}

/** A site's favicon from the cache (never fetched here), or a letter tile. */
export function SiteFavicon({ site }: { site: string }) {
  const [src, setSrc] = useState<string | null>(() => found.get(site) ?? null)

  useEffect(() => {
    let alive = true
    setSrc(found.get(site) ?? null)
    void faviconFor(site).then((dataUrl) => {
      if (alive) setSrc(dataUrl)
    })
    return () => {
      alive = false
    }
  }, [site])

  if (src) return <img className="kb-bm-fav-img" src={src} alt="" draggable={false} />

  const ch = site.charAt(0).toUpperCase() || '?'
  return <span className={cx('kb-bm-fav-tile', FALLBACK[ch.charCodeAt(0) % FALLBACK.length])}>{ch}</span>
}
