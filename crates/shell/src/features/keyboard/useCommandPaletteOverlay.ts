/**
 * Chrome-side orchestration for the command palette overlay.
 *
 * Cmd+L (or any other handler that dispatches the
 * `wis-command-palette-open` DOM event) toggles a full-window overlay
 * webview that hosts `PaletteSurface`. The palette closes itself by
 * calling `hideOverlay('palette')` and broadcasts a Tauri event so this
 * hook resets the toggle state.
 */

import { useCallback, useEffect, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { useOverlay } from '@/hooks/useOverlay'
import { COMMAND_PALETTE_OPEN_EVENT } from '@/features/keyboard/useKeyActions'
import { watchLayout } from '@/lib/layoutWatch'

const PALETTE_FOCUS_EVENT = 'wis-overlay-palette-focus'
const PALETTE_CLOSED_EVENT = 'wis-overlay-palette-closed'

export function useCommandPaletteOverlay(): void {
  const overlay = useOverlay('palette')
  const openRef = useRef(false)

  // The palette centers over the content (webview) area.
  const contentRect = () => {
    const host = document.querySelector('.kb-content-host')
    const r = host?.getBoundingClientRect()
    if (!r || r.width === 0) {
      return { x: 0, y: 0, width: window.innerWidth, height: window.innerHeight }
    }
    return {
      x: Math.round(r.x),
      y: Math.round(r.y),
      width: Math.round(r.width),
      height: Math.round(r.height),
    }
  }

  const open = useCallback(async () => {
    await overlay.show(contentRect())
    openRef.current = true
    await emit(PALETTE_FOCUS_EVENT).catch(() => undefined)
  }, [overlay])

  const close = useCallback(async () => {
    await overlay.hide()
    openRef.current = false
  }, [overlay])

  const toggle = useCallback(() => {
    if (openRef.current) void close()
    else void open()
  }, [open, close])

  useEffect(() => {
    void commands.ensureOverlay('palette').catch(() => undefined)
  }, [])

  useEffect(() => {
    const handler = () => toggle()
    window.addEventListener(COMMAND_PALETTE_OPEN_EVENT, handler)
    return () => window.removeEventListener(COMMAND_PALETTE_OPEN_EVENT, handler)
  }, [toggle])

  useEffect(() => {
    const onResize = () => {
      if (!openRef.current) return
      void overlay.show(contentRect())
    }
    return watchLayout(document.querySelector('.kb-content-host'), onResize)
  }, [overlay])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<void>(PALETTE_CLOSED_EVENT, () => {
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
