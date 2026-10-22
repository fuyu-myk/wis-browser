import { useEffect, useRef } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { WorkspaceId } from '@/bindings/types'
import { useBrowserState } from '@/hooks/useBrowserState'

const OPEN_EXTERNAL_EVENT = 'wis-open-external'

/**
 * Links handed to Wis from other apps (Wis as the default browser). Only the
 * `main` window handles them, and unlike `useOpenInTab` there is no focus
 * gate: an external open can land before the OS finishes activating Wis, so
 * a focus check would drop it. Cold-start URLs queued in the backend are
 * drained once the event listener is live; later opens stream in as
 * `wis-open-external` events. A link a rule routes to another workspace
 * switches the window there first, or brings forward the window showing it.
 */
export function useExternalUrls(): void {
  const { data: state } = useBrowserState()
  const wsRef = useRef<WorkspaceId | null>(null)
  wsRef.current = state?.active_workspace_id ?? null

  useEffect(() => {
    let label: string
    try {
      label = getCurrentWindow().label
    } catch {
      return
    }
    if (label !== 'main') return

    const open = async (href: string) => {
      const current = wsRef.current
      if (!current || !href) return
      const routed = await commands.routeExternalLink(href).catch(() => null)
      const workspace = routed ?? current
      if (workspace !== current) {
        await commands.setActiveWorkspace(workspace).catch(() => undefined)
        wsRef.current = await commands
          .getBrowserState()
          .then((shown) => shown.active_workspace_id)
          .catch(() => wsRef.current)
      }
      await commands.createTab(workspace, href).catch(() => undefined)
    }
    let opening = Promise.resolve()
    const openInTurn = (href: string) => {
      opening = opening.then(() => open(href))
    }

    let unlisten: UnlistenFn | undefined
    let cancelled = false
    void listen<string>(OPEN_EXTERNAL_EVENT, (e) => openInTurn(e.payload)).then((fn) => {
      if (cancelled) {
        fn()
        return
      }
      unlisten = fn
      // Drain only after the listener is live.
      void commands
        .drainExternalUrls()
        .then((urls) => urls.forEach(openInTurn))
        .catch(() => undefined)
    })

    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])
}
