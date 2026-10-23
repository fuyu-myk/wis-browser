/**
 * Chrome-side orchestration for the New Window free-workspace picker overlay.
 *
 * The `new_window` keyaction (menu / accelerator, focus-scoped in
 * useKeyActions) dispatches the `wis-window-picker-open` DOM event, which
 * toggles a content-area overlay webview hosting `WindowPickerSurface`. The
 * surface closes itself via `hideOverlay('windowpicker')` and broadcasts a
 * Tauri event so this hook resets its toggle state.
 */

import { useCallback, useEffect, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { useOverlay } from '@/hooks/useOverlay'
import { watchLayout } from '@/lib/layoutWatch'

export const WINDOW_PICKER_OPEN_EVENT = 'wis-window-picker-open'

const PICKER_FOCUS_EVENT = 'wis-overlay-windowpicker-focus'
const PICKER_CLOSED_EVENT = 'wis-overlay-windowpicker-closed'

export function useWindowPickerOverlay(): void {
  const overlay = useOverlay('windowpicker')
  const openRef = useRef(false)

  // Center over the content (webview) area.
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
    await emit(PICKER_FOCUS_EVENT).catch(() => undefined)
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
    void commands.ensureOverlay('windowpicker').catch(() => undefined)
  }, [])

  useEffect(() => {
    const handler = () => toggle()
    window.addEventListener(WINDOW_PICKER_OPEN_EVENT, handler)
    return () => window.removeEventListener(WINDOW_PICKER_OPEN_EVENT, handler)
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
    void listen<void>(PICKER_CLOSED_EVENT, () => {
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
