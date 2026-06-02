import { useEffect, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { Tab } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { SettingsIcon } from '@/lib/icons'
import { isChromeUrl, SETTINGS_URL } from '@/lib/url'
import { WisMark } from '@/components/WisMark'

const FAVICON_CACHED_EVENT = 'favicon-cached'

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
 * `favicons.rs`) and persisted as `tab.favicon = Some(FaviconRef)`. So
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
  const [iconUrl, setIconUrl] = useState<string | null>(null)
  const [status, setStatus] = useState<Status>('pending')
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
    setStatus('pending')
    clearFailTimer()
    let cancelled = false
    const lookup = async () => {
      let hash = explicitHash
      if (!hash) {
        hash = await commands.faviconOriginHash(tab.url).catch(() => null)
      }
      ownHashRef.current = hash
      if (cancelled) return
      if (!hash) {
        setIconUrl(null)
        setStatus('failed')
        return
      }
      const url = await commands.getFaviconDataUrl(hash).catch(() => null)
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
    void commands
      .getFaviconDataUrl(hash)
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
