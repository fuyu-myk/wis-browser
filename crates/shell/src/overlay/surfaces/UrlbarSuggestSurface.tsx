/**
 * Address-bar suggestions dropdown hosted by the `overlay-urlbar` webview.
 *
 * Pure renderer: it receives a flat, pre-grouped row list + keyboard highlight
 * from the chrome via {@link URLBAR_RENDER_EVENT} and reports row / ✕ clicks
 * back via {@link URLBAR_PICK_EVENT} / {@link URLBAR_REMOVE_EVENT} (flat
 * index). Section headers are inserted wherever a row's `section` label
 * changes. It never takes keyboard focus.
 */

import { Fragment, useEffect, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { cx } from '@/lib/cx'
import { SearchIcon, XIcon } from '@/lib/icons'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { menusGlass } from '@/lib/glass'
import {
  isRemovable,
  URLBAR_PICK_EVENT,
  URLBAR_REMOVE_EVENT,
  URLBAR_RENDER_EVENT,
  type RenderRow,
  type UrlbarRenderPayload,
} from '@/lib/urlbarSuggest'

/** Host portion of a URL for the secondary line, www-stripped. */
function hostOf(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, '')
  } catch {
    return url
  }
}

// Process-wide (per-webview) favicon cache: url -> data URL (or null if the
// origin has none cached). Lookups are read-only against the local favicon
// store populated from history.
const faviconCache = new Map<string, string | null>()

/** A site's cached favicon, falling back to the globe glyph when absent. */
function SiteFavicon({ url }: { url: string }) {
  const [icon, setIcon] = useState<string | null>(() => faviconCache.get(url) ?? null)

  useEffect(() => {
    if (faviconCache.has(url)) {
      setIcon(faviconCache.get(url) ?? null)
      return
    }
    let cancelled = false
    void (async () => {
      const hash = await commands.faviconOriginHash(url).catch(() => null)
      const dataUrl = hash ? await commands.getFaviconDataUrl(hash).catch(() => null) : null
      if (cancelled) return
      faviconCache.set(url, dataUrl)
      setIcon(dataUrl)
    })()
    return () => {
      cancelled = true
    }
  }, [url])

  return icon ? (
    <img className="kb-urlsug-favimg" src={icon} alt="" draggable={false} />
  ) : (
    <GlobeGlyph />
  )
}

export function UrlbarSuggestSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const glassMenus = menusGlass(state?.settings)

  const [rows, setRows] = useState<RenderRow[]>([])
  const [highlight, setHighlight] = useState(-1)
  const [hover, setHover] = useState<number | null>(null)

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<UrlbarRenderPayload>(URLBAR_RENDER_EVENT, (e) => {
      setRows(e.payload.rows)
      setHighlight(e.payload.highlight)
      setHover(null) // a fresh list invalidates the hovered index
    }).then((fn) => (cancelled ? fn() : (unlisten = fn)))
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  if (rows.length === 0) return null

  const active = hover ?? highlight

  return (
    <div className={cx('kb-urlsug', { 'glass-menus': glassMenus })} role="listbox">
      {rows.map((row, index) => {
        const prevSection = index > 0 ? rows[index - 1].section : null
        const header = row.section && row.section !== prevSection ? row.section : null
        return (
          <Fragment key={index}>
            {header && <div className="kb-urlsug-head">{header}</div>}
            <SuggestRowView
              row={row}
              index={index}
              active={active === index}
              onHover={() => setHover(index)}
            />
          </Fragment>
        )
      })}
    </div>
  )
}

interface SuggestRowViewProps {
  row: RenderRow
  index: number
  active: boolean
  onHover: () => void
}

function SuggestRowView({ row, index, active, onHover }: SuggestRowViewProps) {
  const isSite = row.kind === 'site'
  const isSearchQuery = row.kind === 'searchQuery'
  const host = isSite ? hostOf(row.url) : ''
  // For sites: show the page title, falling back to the host; only add the
  // host on a second line when it differs.
  const title = isSite ? (row.title && row.title !== host ? row.title : host) : row.query
  const secondary = isSite
    ? title !== host
      ? host
      : null
    : isSearchQuery
      ? `Search with ${row.engineLabel}`
      : null

  return (
    <div
      className={cx('kb-urlsug-row', { 'is-active': active })}
      role="option"
      aria-selected={active}
      onMouseEnter={onHover}
      onMouseDown={(e) => {
        e.preventDefault()
        void emit(URLBAR_PICK_EVENT, { index }).catch(() => undefined)
      }}
    >
      <span className="kb-urlsug-ico" aria-hidden>
        {row.kind === 'search' ? (
          <ClockGlyph />
        ) : isSearchQuery ? (
          <SearchIcon />
        ) : (
          <SiteFavicon url={row.url} />
        )}
      </span>
      <span className="kb-urlsug-text">
        <span className="kb-urlsug-title">{title}</span>
        {secondary && <span className="kb-urlsug-url">{secondary}</span>}
      </span>
      {isSite && row.openTabId && (
        <span className="kb-urlsug-tab">
          Switch to tab
          <ArrowGlyph />
        </span>
      )}
      {isRemovable(row) && (
        <button
          type="button"
          className="kb-urlsug-x"
          title="Remove from history"
          aria-label="Remove from history"
          onMouseDown={(e) => {
            e.preventDefault()
            e.stopPropagation()
            void emit(URLBAR_REMOVE_EVENT, { index }).catch(() => undefined)
          }}
        >
          <XIcon />
        </button>
      )}
    </div>
  )
}

function ClockGlyph() {
  return (
    <svg
      width={15}
      height={15}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <circle cx="12" cy="12" r="9" />
      <polyline points="12 7 12 12 15.5 14" />
    </svg>
  )
}

function GlobeGlyph() {
  return (
    <svg
      width={15}
      height={15}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18" />
      <path d="M12 3a14 14 0 0 1 0 18 14 14 0 0 1 0-18Z" />
    </svg>
  )
}

function ArrowGlyph() {
  return (
    <svg
      width={12}
      height={12}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2.2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M5 12h14" />
      <path d="m13 6 6 6-6 6" />
    </svg>
  )
}
