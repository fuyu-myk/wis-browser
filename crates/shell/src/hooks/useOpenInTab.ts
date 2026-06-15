import { useEffect, useRef } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { WorkspaceId } from '@/bindings/types'
import { isWindowFocused } from '@/lib/windowFocus'
import { useBrowserState } from '@/hooks/useBrowserState'

const OPEN_IN_TAB_EVENT = 'wis-open-in-tab'

/**
 * Cmd/middle-click a link in a page -> open it in a new background tab. The
 * native `wisOpenInTab` handler emits `wis-open-in-tab` (app-wide), so gate on
 * focus: only the window the user clicked in creates the tab, in its own
 * workspace (`get_browser_state` self-heals `active_workspace_id` to this
 * window's). The opener stays active (`create_background_tab`).
 */
export function useOpenInTab(): void {
  const { data: state } = useBrowserState()
  const wsRef = useRef<WorkspaceId | null>(null)
  wsRef.current = state?.active_workspace_id ?? null

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<string>(OPEN_IN_TAB_EVENT, (e) => {
      if (!isWindowFocused()) return
      const ws = wsRef.current
      const href = e.payload
      if (!ws || !href) return
      void commands.createBackgroundTab(ws, href).catch(() => undefined)
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
