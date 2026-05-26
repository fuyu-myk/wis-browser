import { useEffect, useMemo, useRef } from 'react'

import type { BrowserState, Tab } from '@/bindings/types'
import { isChromeUrl } from '@/lib/url'
import { useTabWebviews } from '@/hooks/useTabWebviews'

import { NewTabPage } from '@/components/NewTabPage'

interface TabWebViewHostProps {
  state: BrowserState
  /** When true, all native tab webviews are parked off-screen so HTML
   *  chrome (e.g. the settings modal) is visible without OS-overlay
   *  interference. Used for full-window modals only: sidebar-anchored
   *  popovers are clamped to chrome space instead. */
  suppressWebview?: boolean
}

export function TabWebViewHost({ state, suppressWebview }: TabWebViewHostProps) {
  const hostRef = useRef<HTMLDivElement | null>(null)

  const activeWorkspace = state.workspaces.find(
    (w) => w.id === state.active_workspace_id,
  )

  const workspaceTabs = useMemo<Tab[]>(() => {
    if (!activeWorkspace) return []
    return activeWorkspace.tab_order
      .map((id) => state.tabs.find((t) => t.id === id))
      .filter((t): t is Tab => t !== undefined)
  }, [activeWorkspace, state.tabs])

  const activeTab =
    activeWorkspace?.active_tab_id != null
      ? workspaceTabs.find((t) => t.id === activeWorkspace.active_tab_id) ?? null
      : null
  const showNewTabPage = !activeTab || isChromeUrl(activeTab.url)

  const { setBounds } = useTabWebviews({
    tabs: workspaceTabs,
    activeTabId: suppressWebview ? null : activeTab?.id ?? null,
    activeWorkspaceId: activeWorkspace?.id ?? null,
  })

  useEffect(() => {
    const el = hostRef.current
    if (!el) return
    const push = () => {
      const rect = el.getBoundingClientRect()
      setBounds({
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
      })
    }
    push()
    const obs = new ResizeObserver(push)
    obs.observe(el)

    window.addEventListener('resize', push)
    return () => {
      obs.disconnect()
      window.removeEventListener('resize', push)
    }
  }, [setBounds])

  return (
    <div ref={hostRef} className="kb-content-host">
      {showNewTabPage && <NewTabPage activeTab={activeTab} />}
    </div>
  )
}
