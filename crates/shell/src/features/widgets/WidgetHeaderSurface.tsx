/**
 * Slim accent-coloured header bar docked to the top of the visible widget
 * panel, hosting a mute toggle and a reload button (top right). Lives in its
 * own overlay webview because overlays are the only chrome UI promoted above
 * native webviews. It only reports clicks; the main chrome owns which widget
 * is visible, applies the engine-level page mute (so messaging pings stay
 * silent too), and triggers the native (cache-bypassing) reload, which works
 * even when the page inside the panel is hung.
 */

import { useEffect, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import {
  WPANEL_MUTE_EVENT,
  WPANEL_RELOAD_EVENT,
  WPANEL_STATE_EVENT,
  type WPanelStatePayload,
} from '@/features/widgets/widgetResize'

/** Display host of a URL: authority without scheme, `www.`, or path. */
function hostOf(url: string): string {
  if (!url) return ''
  try {
    return new URL(url).host.replace(/^www\./, '')
  } catch {
    return ''
  }
}

export function WidgetHeaderSurface() {
  useBrowserStateEvents()
  const { data: state } = useBrowserState()
  useThemeApply(state?.settings)

  // Mirrors the visible widget's state (mute + current URL), pushed by the
  // main chrome on every show, navigation, and mute toggle.
  const [muted, setMuted] = useState(false)
  const [url, setUrl] = useState('')
  useEffect(() => {
    let cancelled = false
    let unlisten: UnlistenFn | null = null
    void listen<WPanelStatePayload>(WPANEL_STATE_EVENT, (e) => {
      setMuted(e.payload.muted)
      setUrl(e.payload.url)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  return (
    <div className="kb-wheader-surface">
      <span className="kb-wheader-host" title={url}>
        {hostOf(url)}
      </span>
      <button
        type="button"
        className="kb-wheader-mute"
        title={muted ? 'Unmute' : 'Mute'}
        aria-label={muted ? 'Unmute widget' : 'Mute widget'}
        onClick={() => void emit(WPANEL_MUTE_EVENT).catch(() => undefined)}
      >
        {muted ? (
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden
          >
            <path d="M11 4.7 6.6 8.2H3v7.6h3.6L11 19.3z" />
            <path d="m21 9-6 6" />
            <path d="m15 9 6 6" />
          </svg>
        ) : (
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.2"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden
          >
            <path d="M11 4.7 6.6 8.2H3v7.6h3.6L11 19.3z" />
            <path d="M15.5 8.5a5 5 0 0 1 0 7" />
            <path d="M18.6 5.4a9 9 0 0 1 0 13.2" />
          </svg>
        )}
      </button>
      <button
        type="button"
        className="kb-wheader-reload"
        title="Reload"
        aria-label="Reload widget"
        onClick={() => void emit(WPANEL_RELOAD_EVENT).catch(() => undefined)}
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden
        >
          <path d="M21 12a9 9 0 1 1-2.64-6.36" />
          <path d="M21 3v6h-6" />
        </svg>
      </button>
    </div>
  )
}
