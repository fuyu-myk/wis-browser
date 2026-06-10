/**
 * Chrome-side orchestration for the downloads popup overlay, mirroring
 * `useBookmarksOverlay`: the downloads glyph dispatches `DOWNLOADS_OPEN_EVENT`
 * (a DOM event) to toggle a full-window `overlay-downloads` webview hosting
 * `DownloadsMenuSurface`; the surface closes itself via
 * `hideOverlay('downloads')` and broadcasts `DOWNLOADS_CLOSED_EVENT` so this
 * hook resets its toggle state.
 */

import { useCallback, useEffect, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { useOverlay } from '@/hooks/useOverlay'
import { DOWNLOADS_CLOSED_EVENT, DOWNLOADS_OPEN_EVENT } from '@/lib/downloads'

export function useDownloadsOverlay(): void {
  const overlay = useOverlay('downloads')
  const openRef = useRef(false)

  const fullRect = () => ({
    x: 0,
    y: 0,
    width: window.innerWidth,
    height: window.innerHeight,
  })

  const open = useCallback(async () => {
    await overlay.show(fullRect())
    openRef.current = true
    // Force the popup to refetch on every open so it never shows stale data.
    void emit('browser-state-changed').catch(() => undefined)
    void emit('downloads-changed').catch(() => undefined)
  }, [overlay])

  const close = useCallback(async () => {
    await overlay.hide()
    openRef.current = false
  }, [overlay])

  useEffect(() => {
    const handler = () => {
      if (openRef.current) void close()
      else void open()
    }
    window.addEventListener(DOWNLOADS_OPEN_EVENT, handler)
    return () => window.removeEventListener(DOWNLOADS_OPEN_EVENT, handler)
  }, [open, close])

  useEffect(() => {
    const onResize = () => {
      if (openRef.current) void overlay.show(fullRect())
    }
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [overlay])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<void>(DOWNLOADS_CLOSED_EVENT, () => {
      openRef.current = false
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])
}
