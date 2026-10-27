import { useEffect, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { Tab } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { CanvasIcon, SettingsIcon, UsageIcon } from '@/lib/icons'
import { HISTORY_URL, isCanvasUrl, isChromeUrl, SETTINGS_URL, USAGE_URL } from '@/lib/url'
import { WisMark } from '@/components/WisMark'

const FAVICON_CACHED_EVENT = 'favicon-cached'

// Process-wide favicon caches.
const dataUrlCache = new Map<string, string>()
const dataUrlInflight = new Map<string, Promise<string | null>>()
const originHashCache = new Map<string, string | null>()

/** Resolve a favicon hash to a data URL once, deduping concurrent lookups. */
function resolveDataUrl(hash: string): Promise<string | null> {
  const cached = dataUrlCache.get(hash)
  if (cached !== undefined) return Promise.resolve(cached)
  let inflight = dataUrlInflight.get(hash)
  if (!inflight) {
    inflight = commands
      .getFaviconDataUrl(hash)
      .catch(() => null)
      .then((url) => {
        dataUrlInflight.delete(hash)
        if (url) dataUrlCache.set(hash, url)
        return url
      })
    dataUrlInflight.set(hash, inflight)
  }
  return inflight
}

interface TabFaviconProps {
  tab: Tab
  /** Extra class to apply to the wrapping element (e.g. `fav` vs sizing). */
  className?: string
}

const FALLBACK_PALETTES = ['fav-pri', 'fav-mint', 'fav-rose', 'fav-sun', 'fav-lila', 'fav-azur']

/**
 * Renders a tab's site favicon from the local content-addressed cache,
 * falling back to a coloured-letter tile when nothing is cached yet
 * (chrome URLs, fresh tab pre-capture, unsupported sites).
 *
 * The favicon is captured Rust-side from `on_page_load(Finished)` (see
 * `data/favicons.rs`) and persisted as `tab.favicon = Some(FaviconRef)`. So
 * the render path here is purely a `hash -> data: URL` lookup with no
 * outbound traffic to Google or any other third party.
 */
type Status = 'pending' | 'loaded' | 'failed'

/**
 * Upper bound on how long we wait for an in-flight capture before
 * giving up and showing the letter tile.
 */
const FAVICON_PENDING_TIMEOUT_MS = 6500

export function TabFavicon({ tab, className }: TabFaviconProps) {
  const explicitHash = tab.favicon?.hash ?? null
  const cachedInitial = explicitHash ? dataUrlCache.get(explicitHash) ?? null : null
  const [iconUrl, setIconUrl] = useState<string | null>(cachedInitial)
  const [status, setStatus] = useState<Status>(cachedInitial ? 'loaded' : 'pending')
  const ownHashRef = useRef<string | null>(explicitHash)
  const failTimerRef = useRef<number | null>(null)

  const armFailTimer = () => {
    if (failTimerRef.current !== null) return
    failTimerRef.current = window.setTimeout(() => {
      failTimerRef.current = null
      setStatus('failed')
    }, FAVICON_PENDING_TIMEOUT_MS)
  }

  const clearFailTimer = () => {
    if (failTimerRef.current !== null) {
      window.clearTimeout(failTimerRef.current)
      failTimerRef.current = null
    }
  }

  useEffect(() => {
    const cacheHit = explicitHash ? dataUrlCache.get(explicitHash) : undefined
    if (cacheHit !== undefined) {
      ownHashRef.current = explicitHash
      setIconUrl(cacheHit)
      setStatus('loaded')
      return
    }

    setStatus('pending')
    clearFailTimer()
    let cancelled = false
    const lookup = async () => {
      let hash = explicitHash
      if (!hash) {
        const memo = originHashCache.get(tab.url)
        if (memo !== undefined) {
          hash = memo
        } else {
          hash = await commands.faviconOriginHash(tab.url).catch(() => null)
          if (!cancelled) originHashCache.set(tab.url, hash)
        }
      }
      ownHashRef.current = hash
      if (cancelled) return
      if (!hash) {
        setIconUrl(null)
        setStatus('failed')
        return
      }
      const url = await resolveDataUrl(hash)
      if (cancelled) return
      if (url) {
        setIconUrl(url)
        setStatus('loaded')
        return
      }
      armFailTimer()
    }
    void lookup()
    return () => {
      cancelled = true
      clearFailTimer()
    }
  }, [explicitHash, tab.url])

  useEffect(() => {
    if (tab.loading) return
    const hash = ownHashRef.current
    if (!hash) return
    let cancelled = false
    void resolveDataUrl(hash)
      .then((url) => {
        if (cancelled || !url) return
        clearFailTimer()
        setIconUrl(url)
        setStatus('loaded')
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
    }
  }, [tab.loading])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<string>(FAVICON_CACHED_EVENT, (e) => {
      if (e.payload !== ownHashRef.current) return
      void commands
        .getFaviconDataUrl(e.payload)
        .then((url) => {
          if (url) dataUrlCache.set(e.payload, url)
          if (cancelled) return
          clearFailTimer()
          setIconUrl(url)
          setStatus(url ? 'loaded' : 'failed')
        })
        .catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  useEffect(() => {
    return () => clearFailTimer()
  }, [])

  if (tab.url === SETTINGS_URL) {
    return (
      <span
        className={cx('fav', 'fav-settings', className)}
        aria-hidden
      >
        <SettingsIcon />
      </span>
    )
  }

  if (tab.url === HISTORY_URL) {
    return (
      <span className={cx('fav', 'fav-history', className)} aria-hidden>
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden
        >
          <circle cx="12" cy="12" r="9" />
          <path d="M12 7v5l3 2" />
        </svg>
      </span>
    )
  }

  if (tab.url === USAGE_URL) {
    return (
      <span className={cx('fav', 'fav-usage', className)} aria-hidden>
        <UsageIcon aria-hidden />
      </span>
    )
  }

  if (isCanvasUrl(tab.url)) {
    return (
      <span className={cx('fav', 'fav-canvas', className)} aria-hidden>
        <CanvasIcon aria-hidden />
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
        role="img"
        aria-label="Loading"
      >
        <WisMark variant="loading" size="100%" />
      </span>
    )
  }

  if (status === 'pending') {
    return <span className={cx('fav', 'fav-pending', className)} aria-hidden />
  }

  if (status === 'loaded' && iconUrl) {
    return (
      <span className={cx('fav', className)}>
        <img
          src={iconUrl}
          alt=""
          onError={() => setStatus('failed')}
          draggable={false}
        />
      </span>
    )
  }

  return (
    <span className={cx('fav', fallbackClass(tab), className)}>
      {tabInitial(tab)}
    </span>
  )
}

function fallbackClass(tab: Tab): string {
  const seed = (tab.title[0] ?? tab.url[0] ?? '?').charCodeAt(0)
  return FALLBACK_PALETTES[seed % FALLBACK_PALETTES.length]
}

function tabInitial(tab: Tab): string {
  const source = tab.title.trim() || tab.url
  return (source[0] ?? '•').toUpperCase()
}
