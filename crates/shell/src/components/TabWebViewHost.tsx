import { useCallback, useEffect, useLayoutEffect, useMemo, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import type { BrowserState, KeyAction, Tab } from '@/bindings/types'
import { isChromeUrl, SETTINGS_URL } from '@/lib/url'
import { useOverlay } from '@/hooks/useOverlay'
import { useTabWebviews } from '@/hooks/useTabWebviews'

import { FOCUS_LAUNCHPAD_SEARCH_EVENT, NewTabPage } from '@/components/NewTabPage'
import { SettingsPage } from '@/components/SettingsPage'

const KEYACTION_EVENT = 'wis-keyaction'
const FIND_FOCUS_EVENT = 'wis-overlay-find-focus'
const FIND_CLOSE_REQUEST_EVENT = 'wis-overlay-find-close'

const FIND_STRIP_HEIGHT_PX = 80

interface TabWebViewHostProps {
  state: BrowserState
}

export function TabWebViewHost({ state }: TabWebViewHostProps) {
  const hostRef = useRef<HTMLDivElement | null>(null)
  const findOverlay = useOverlay('find')
  const findOpenRef = useRef(false)

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
  const isLaunchpadRef = useRef(false)
  isLaunchpadRef.current = isChrome && !isSettings

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

  // Compute the bottom strip's bounds in window coordinates from the
  // content host. The find overlay surface fills this strip.
  const findRect = useCallback(() => {
    const el = hostRef.current
    if (!el) return null
    const r = el.getBoundingClientRect()
    return {
      x: r.x,
      y: r.y + r.height - FIND_STRIP_HEIGHT_PX,
      width: r.width,
      height: FIND_STRIP_HEIGHT_PX,
    }
  }, [])

  // Find-overlay orchestration: open on Cmd+F (find_in_page action), hide
  // when the surface requests close, keep the strip pinned to the bottom
  // on window resize.
  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []

    const openFind = async () => {
      const rect = findRect()
      if (!rect) return
      await findOverlay.show(rect)
      findOpenRef.current = true
      // Surface listens for this after it mounts to focus its input.
      await emit(FIND_FOCUS_EVENT)
    }
    const closeFind = async () => {
      findOpenRef.current = false
      await findOverlay.hide()
    }

    void listen<{ action: KeyAction }>(KEYACTION_EVENT, (e) => {
      if (e.payload.action !== 'find_in_page') return
      if (isLaunchpadRef.current) {
        window.dispatchEvent(new CustomEvent(FOCUS_LAUNCHPAD_SEARCH_EVENT))
        return
      }
      void openFind()
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    void listen<void>(FIND_CLOSE_REQUEST_EVENT, () => {
      void closeFind()
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    const onResize = () => {
      if (!findOpenRef.current) return
      const rect = findRect()
      if (rect) void findOverlay.show(rect)
    }
    window.addEventListener('resize', onResize)

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
      window.removeEventListener('resize', onResize)
    }
  }, [findOverlay, findRect])

  // Close find when the user switches tabs.
  useEffect(() => {
    if (findOpenRef.current) {
      findOpenRef.current = false
      void findOverlay.hide().catch(() => undefined)
    }
  }, [activeTab?.id, findOverlay])

  return (
    <div ref={hostRef} className="kb-content-host">
      {isChrome && (isSettings ? <SettingsPage /> : <NewTabPage activeTab={activeTab} />)}
    </div>
  )
}
