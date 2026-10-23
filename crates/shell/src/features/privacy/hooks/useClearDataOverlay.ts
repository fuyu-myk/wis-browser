/**
 * Chrome-side orchestration for the "Clear browsing data" overlay.
 *
 * The menu / palette / settings dispatch the `wis-open-clear-data` DOM event;
 * this hook shows a full-window overlay webview hosting `ClearDataSurface` so
 * the dialog floats above the page webviews. The surface closes itself
 * via `hideOverlay('cleardata')` and broadcasts a Tauri event so this hook resets.
 */

import { useCallback, useEffect, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { useOverlay } from '@/hooks/useOverlay'

/** DOM event (menu/palette/settings) that opens the dialog. */
export const OPEN_CLEAR_DATA_EVENT = 'wis-open-clear-data'
const CLEAR_DATA_FOCUS_EVENT = 'wis-overlay-cleardata-focus'
const CLEAR_DATA_CLOSED_EVENT = 'wis-overlay-cleardata-closed'

export function useClearDataOverlay(): void {
  const overlay = useOverlay('cleardata')
  const openRef = useRef(false)

  const open = useCallback(async () => {
    await overlay.show({ x: 0, y: 0, width: window.innerWidth, height: window.innerHeight })
    openRef.current = true
    await emit(CLEAR_DATA_FOCUS_EVENT).catch(() => undefined)
  }, [overlay])

  useEffect(() => {
    const handler = () => void open()
    window.addEventListener(OPEN_CLEAR_DATA_EVENT, handler)
    return () => window.removeEventListener(OPEN_CLEAR_DATA_EVENT, handler)
  }, [open])

  useEffect(() => {
    const onResize = () => {
      if (!openRef.current) return
      void overlay.show({ x: 0, y: 0, width: window.innerWidth, height: window.innerHeight })
    }
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [overlay])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<void>(CLEAR_DATA_CLOSED_EVENT, () => {
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
