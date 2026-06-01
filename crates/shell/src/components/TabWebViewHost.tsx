import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'

import type { BrowserState, Tab } from '@/bindings/types'
import { isChromeUrl, SETTINGS_URL } from '@/lib/url'
import { FIND_CLOSE_EVENT, FIND_OPEN_EVENT } from '@/hooks/useKeyActions'
import { useTabWebviews } from '@/hooks/useTabWebviews'

import { FindBar, FINDBAR_GAP_PX } from '@/components/FindBar'
import { NewTabPage } from '@/components/NewTabPage'
import { SettingsPage } from '@/components/SettingsPage'

interface TabWebViewHostProps {
  state: BrowserState
}

export function TabWebViewHost({ state }: TabWebViewHostProps) {
  const hostRef = useRef<HTMLDivElement | null>(null)
  // Tracks the FindBar's visibility to shrink the WKWebView bounds.
  const [findOpen, setFindOpen] = useState(false)
  useEffect(() => {
    const open = () => setFindOpen(true)
    const close = () => setFindOpen(false)
    window.addEventListener(FIND_OPEN_EVENT, open)
    window.addEventListener(FIND_CLOSE_EVENT, close)
    return () => {
      window.removeEventListener(FIND_OPEN_EVENT, open)
      window.removeEventListener(FIND_CLOSE_EVENT, close)
    }
  }, [])

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
  const isChrome = !activeTab || isChromeUrl(activeTab.url)
  const isSettings = activeTab?.url === SETTINGS_URL

  const { setBounds } = useTabWebviews({
    tabs: workspaceTabs,
    activeTabId: isChrome ? null : activeTab?.id ?? null,
    activeWorkspaceId: activeWorkspace?.id ?? null,
  })

  useLayoutEffect(() => {
    const el = hostRef.current
    if (!el) return
    const push = () => {
      const rect = el.getBoundingClientRect()
      const gap = findOpen ? FINDBAR_GAP_PX : 0
      setBounds({
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: Math.max(0, rect.height - gap),
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
  }, [setBounds, findOpen])

  return (
    <div ref={hostRef} className="kb-content-host">
      {isChrome && (isSettings ? <SettingsPage /> : <NewTabPage activeTab={activeTab} />)}
      {!isChrome && <FindBar activeTab={activeTab} />}
    </div>
  )
}
