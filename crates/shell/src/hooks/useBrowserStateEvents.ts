/**
 * Listens for the Rust-side `browser-state-changed` Tauri event and
 * invalidates the cached `BrowserState` query so React-Query refetches.
 *
 * The event is fired from `commands.rs` whenever a wry navigation
 * callback (URL / title / loading) successfully mutates the state. A
 * single coarse event is enough for now: the chrome already refetches
 * the entire `BrowserState` on every mutation.
 *
 * Mount this hook exactly once at the top of the React tree (see
 * `App.tsx`).
 */

import { useEffect } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useQueryClient } from '@tanstack/react-query'

import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

const EVENT_NAME = 'browser-state-changed'

export function useBrowserStateEvents(): void {
  const qc = useQueryClient()
  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    let frame: number | null = null

    void listen<void>(EVENT_NAME, () => {
      if (frame !== null) return
      frame = requestAnimationFrame(() => {
        frame = null
        qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
      })
    }).then((fn) => {
      if (cancelled) {
        fn()
      } else {
        unlisten = fn
      }
    })

    return () => {
      cancelled = true
      if (frame !== null) cancelAnimationFrame(frame)
      unlisten?.()
    }
  }, [qc])
}
