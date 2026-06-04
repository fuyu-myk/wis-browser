import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { BrowserState, KeyAction, PaneBranch, PaneNode, SplitDir, Tab, TabId } from '@/bindings/types'
import { isChromeUrl, SETTINGS_URL } from '@/lib/url'
import { useOverlay } from '@/hooks/useOverlay'
import { useTabWebviews, type Bounds, type WebviewLayout } from '@/hooks/useTabWebviews'

import { FOCUS_LAUNCHPAD_SEARCH_EVENT } from '@/components/NewTabPage'
import { PaneView, type PaneSide } from '@/components/SplitPaneHost'

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

  const tabsById = useMemo(() => {
    const map = new Map<TabId, Tab>()
    for (const t of workspaceTabs) map.set(t.id, t)
    return map
  }, [workspaceTabs])

  const activeTab =
    activeWorkspace?.active_tab_id != null
      ? workspaceTabs.find((t) => t.id === activeWorkspace.active_tab_id) ?? null
      : null
  const isChrome = !activeTab || isChromeUrl(activeTab.url)
  const isSettings = activeTab?.url === SETTINGS_URL
  const isLaunchpadRef = useRef(false)
  isLaunchpadRef.current = isChrome && !isSettings

  const effectiveTree = useMemo<PaneNode | null>(() => {
    if (activeWorkspace?.pane_tree) return activeWorkspace.pane_tree
    return activeTab ? { kind: 'leaf', tab: activeTab.id } : null
  }, [activeWorkspace?.pane_tree, activeTab])
  const treeKey = useMemo(() => JSON.stringify(effectiveTree), [effectiveTree])

  const createTabs = useMemo(
    () => workspaceTabs.filter((t) => !isChromeUrl(t.url)),
    [workspaceTabs],
  )

  const { syncLayout } = useTabWebviews({
    createTabs,
    activeWorkspaceId: activeWorkspace?.id ?? null,
    allTabs: state.tabs,
  })

  const focusPane = useCallback(
    (tabId: TabId) => {
      if (tabId === activeWorkspace?.active_tab_id) return
      void commands.setActiveTab(tabId).catch(() => undefined)
    },
    [activeWorkspace?.active_tab_id],
  )

  const commitRatio = useCallback(
    (path: PaneBranch[], ratio: number) => {
      if (!activeWorkspace) return
      void commands.setPaneRatio(activeWorkspace.id, path, ratio).catch(() => undefined)
    },
    [activeWorkspace],
  )

  // While a divider is being dragged the webviews are hidden.
  // The drag tracks the pointer via `pointermove`.
  const [resizing, setResizing] = useState(false)
  const resizingRef = useRef(false)
  useEffect(() => {
    const onResizeEvent = (e: Event) => {
      const active = (e as CustomEvent).detail === true
      resizingRef.current = active
      setResizing(active)
    }
    window.addEventListener('wis-pane-resize', onResizeEvent)
    return () => window.removeEventListener('wis-pane-resize', onResizeEvent)
  }, [])

  const [draggingTab, setDraggingTab] = useState<TabId | null>(null)
  const draggingRef = useRef<TabId | null>(null)
  draggingRef.current = draggingTab
  useEffect(() => {
    const onBegin = (e: Event) => setDraggingTab((e as CustomEvent).detail as TabId)
    window.addEventListener('wis-tabdrag-begin', onBegin)
    return () => window.removeEventListener('wis-tabdrag-begin', onBegin)
  }, [])
  useEffect(() => {
    if (!draggingTab) return
    // End the drag on release anywhere. A drop on a pane fires its own handler
    // first (pointerup bubbles target → window).
    const onUp = () => setDraggingTab(null)
    window.addEventListener('pointerup', onUp)
    return () => window.removeEventListener('pointerup', onUp)
  }, [draggingTab])

  const handlePaneDrop = useCallback(
    (targetTabId: TabId, movedTabId: TabId, side: PaneSide) => {
      setDraggingTab(null)
      if (!activeWorkspace || targetTabId === movedTabId) return
      if (side === 'center') {
        void commands
          .replacePaneLeaf(activeWorkspace.id, targetTabId, movedTabId)
          .catch(() => undefined)
        return
      }
      const dir: SplitDir = side === 'left' || side === 'right' ? 'horizontal' : 'vertical'
      const placeNewFirst = side === 'left' || side === 'top'
      void commands
        .splitPaneWith(activeWorkspace.id, targetTabId, dir, placeNewFirst, movedTabId)
        .catch(() => undefined)
    },
    [activeWorkspace],
  )

  // Measure each webview pane-leaf and tile its native webview into place.
  const focusWebviewId =
    activeTab && !isChromeUrl(activeTab.url) ? activeTab.id : null
  useLayoutEffect(() => {
    const host = hostRef.current
    if (!host) return
    const measure = () => {
      if (resizingRef.current || draggingRef.current) {
        syncLayout(new Map<TabId, Bounds>(), null)
        return
      }
      const layout: WebviewLayout = new Map<TabId, Bounds>()
      host.querySelectorAll<HTMLElement>('[data-pane-webview]').forEach((el) => {
        const id = el.getAttribute('data-pane-leaf') as TabId | null
        if (!id) return
        const r = el.getBoundingClientRect()
        layout.set(id, { x: r.x, y: r.y, width: r.width, height: r.height })
      })
      syncLayout(layout, focusWebviewId)
    }
    measure()

    const obs = new ResizeObserver(measure)
    obs.observe(host)
    host
      .querySelectorAll<HTMLElement>('[data-pane-leaf]')
      .forEach((el) => obs.observe(el))

    const onResize = () => measure()
    window.addEventListener('resize', onResize)

    // Re-measure when the sidebar's expand/collapse transition ends.
    const onTransitionEnd = (e: TransitionEvent) => {
      const t = e.target as HTMLElement | null
      if (!t?.classList) return
      if (
        (t.classList.contains('kb-side') || t.classList.contains('kb-main')) &&
        (e.propertyName === 'width' || e.propertyName.startsWith('padding'))
      ) {
        measure()
      }
    }
    document.addEventListener('transitionend', onTransitionEnd, true)

    return () => {
      obs.disconnect()
      window.removeEventListener('resize', onResize)
      document.removeEventListener('transitionend', onTransitionEnd, true)
    }
  }, [treeKey, focusWebviewId, syncLayout, resizing, draggingTab])

  // Compute the bottom strip's bounds in window coordinates from the content
  // host. The find overlay surface fills this strip.
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

  // Find-overlay orchestration: open on Cmd+F, hide when the surface requests
  // close, keep the strip pinned to the bottom on window resize.
  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []

    const openFind = async () => {
      const rect = findRect()
      if (!rect) return
      await findOverlay.show(rect)
      findOpenRef.current = true
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

  // Close find when the user switches the focused tab.
  useEffect(() => {
    if (findOpenRef.current) {
      findOpenRef.current = false
      void findOverlay.hide().catch(() => undefined)
    }
  }, [activeTab?.id, findOverlay])

  // Native click-to-focus: the pane-focus monitor emits the tab id of whichever
  // webview was clicked.
  const activeIdRef = useRef<TabId | null>(null)
  activeIdRef.current = activeWorkspace?.active_tab_id ?? null
  useEffect(() => {
    let cancelled = false
    let unlisten: UnlistenFn | null = null
    void listen<string>('wis-pane-focus', (e) => {
      const id = e.payload as TabId
      if (id === activeIdRef.current) return
      void commands.setActiveTab(id).catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  return (
    <div ref={hostRef} className="kb-content-host">
      {effectiveTree ? (
        <PaneView
          node={effectiveTree}
          path={[]}
          tabsById={tabsById}
          focusedId={activeWorkspace?.active_tab_id ?? null}
          onFocusPane={focusPane}
          onRatioCommit={commitRatio}
          dragActive={!!draggingTab}
          movedTabId={draggingTab}
          onPaneDrop={handlePaneDrop}
        />
      ) : null}
    </div>
  )
}
