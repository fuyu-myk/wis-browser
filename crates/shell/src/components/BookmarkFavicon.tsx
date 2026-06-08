import { useEffect, useState } from 'react'

import { commands } from '@/bindings/commands'
import type { Bookmark } from '@/bindings/types'
import { cx } from '@/lib/cx'

const FALLBACK = ['fav-pri', 'fav-mint', 'fav-rose', 'fav-sun', 'fav-lila', 'fav-azur']

/** A bookmark's favicon (cached lookup), with a letter-tile fallback. */
export function BookmarkFavicon({ bm }: { bm: Bookmark }) {
  const [src, setSrc] = useState<string | null>(null)

  useEffect(() => {
    let alive = true
    const load = async () => {
      let hash = bm.favicon_hash
      if (!hash) hash = await commands.faviconOriginHash(bm.url).catch(() => null)
      if (!hash) return
      const dataUrl = await commands.getFaviconDataUrl(hash).catch(() => null)
      if (alive) setSrc(dataUrl)
    }
    void load()
    return () => {
      alive = false
    }
  }, [bm.favicon_hash, bm.url])

  if (src) return <img className="kb-bm-fav-img" src={src} alt="" draggable={false} />

  const label = (bm.title || bm.url).trim()
  const ch = label ? label[0].toUpperCase() : '?'
  const palette = FALLBACK[ch.charCodeAt(0) % FALLBACK.length]
  return <span className={cx('kb-bm-fav-tile', palette)}>{ch}</span>
}
