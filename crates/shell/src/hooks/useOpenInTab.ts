import { useEffect, useRef } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { WorkspaceId } from '@/bindings/types'
import { isWindowFocused } from '@/lib/windowFocus'
import { useBrowserState } from '@/hooks/useBrowserState'

const OPEN_IN_TAB_EVENT = 'wis-open-in-tab'
const OPEN_TAB_FOREGROUND_EVENT = 'wis-open-in-tab-fg'

/**
 * Links a page hands to the chrome. Cmd/middle-click -> a new **background**
 * tab (`wis-open-in-tab`); an engine new-window request that is a plain
 * navigation (`target=_blank`, featureless `window.open`) -> a new
 * **foreground** tab (`wis-open-in-tab-fg`, denied as a popup in
 * `on_new_window`). Either way the tab lands beside its opener. Both events
 * are app-wide, so gate on focus: only the window the user clicked in creates
 * the tab, in its own workspace (`get_browser_state` self-heals
 * `active_workspace_id` to this window's).
 */
export function useOpenInTab(): void {
  const { data: state } = useBrowserState()
  const wsRef = useRef<WorkspaceId | null>(null)
  wsRef.current = state?.active_workspace_id ?? null

  useEffect(() => {
    const unlistens: UnlistenFn[] = []
    let cancelled = false
    const hook = (event: string, open: (ws: WorkspaceId, href: string) => Promise<unknown>) => {
      void listen<string>(event, (e) => {
        if (!isWindowFocused()) return
        const ws = wsRef.current
        const href = e.payload
        if (!ws || !href) return
        void open(ws, href).catch(() => undefined)
      }).then((fn) => {
        if (cancelled) fn()
        else unlistens.push(fn)
      })
    }
    hook(OPEN_IN_TAB_EVENT, (ws, href) => commands.createBackgroundTab(ws, href))
    hook(OPEN_TAB_FOREGROUND_EVENT, (ws, href) => commands.createRelatedTab(ws, href))
    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
    }
  }, [])
}
