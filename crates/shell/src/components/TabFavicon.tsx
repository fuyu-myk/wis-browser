import { useEffect, useState } from 'react'

import type { Tab } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { SettingsIcon } from '@/lib/icons'
import { isChromeUrl, SETTINGS_URL } from '@/lib/url'
import { WisMark } from '@/components/WisMark'

interface TabFaviconProps {
  tab: Tab
  /** Extra class to apply to the wrapping element (e.g. `fav` vs sizing). */
  className?: string
}

const FALLBACK_PALETTES = ['fav-pri', 'fav-mint', 'fav-rose', 'fav-sun', 'fav-lila', 'fav-azur']

/**
 * Renders a tab's site favicon (fetched via Google's public S2 service),
 * falling back to the existing coloured-letter tile when the icon can't
 * be loaded (chrome URLs, network errors, opaque CSPs).
 *
 * The favicon URL is derived from `tab.url` directly;
 * switching tab URLs naturally re-fetches.
 */
export function TabFavicon({ tab, className }: TabFaviconProps) {
  const iconUrl = faviconFor(tab.url)
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    setFailed(false)
  }, [iconUrl])

  if (tab.url === SETTINGS_URL) {
    return (
      <span
        className={cx('fav', 'fav-settings', className)}
        aria-label="settings"
      >
        <SettingsIcon />
      </span>
    )
  }

  if (isChromeUrl(tab.url)) {
    return (
      <span
        className={cx('fav', 'fav-wis', tab.loading && 'is-loading', className)}
        aria-label="wis launchpad"
      >
        <WisMark variant={tab.loading ? 'loading' : 'line-open'} size="100%" />
        {tab.loading && <span className="kb-favicon-spinner" aria-hidden />}
      </span>
    )
  }

  // Loading tabs show the wis owl blinking.
  if (tab.loading) {
    return (
      <span
        className={cx('fav', 'fav-wis', 'is-loading', className)}
        aria-label="loading"
      >
        <WisMark variant="loading" size="100%" />
      </span>
    )
  }

  const showImage = iconUrl && !failed

  return (
    <span
      className={cx('fav', !showImage && fallbackClass(tab), className)}
    >
      {showImage ? (
        <img
          src={iconUrl}
          alt=""
          onError={() => setFailed(true)}
          draggable={false}
        />
      ) : (
        tabInitial(tab)
      )}
    </span>
  )
}

function faviconFor(url: string): string | null {
  try {
    const u = new URL(url)
    if (u.protocol === 'about:' || u.protocol === 'data:' || !u.host) return null
    return `https://www.google.com/s2/favicons?domain=${u.host}&sz=64`
  } catch {
    return null
  }
}

function fallbackClass(tab: Tab): string {
  const seed = (tab.title[0] ?? tab.url[0] ?? '?').charCodeAt(0)
  return FALLBACK_PALETTES[seed % FALLBACK_PALETTES.length]
}

function tabInitial(tab: Tab): string {
  const source = tab.title.trim() || tab.url
  return (source[0] ?? '•').toUpperCase()
}
