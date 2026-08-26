import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { BrowserState, KeyAction, PaneBranch, PaneNode, SplitDir, Tab, TabId } from '@/bindings/types'
import { isChromeUrl, SETTINGS_URL } from '@/lib/url'
import { currentPlatform } from '@/lib/platform'
import { isWindowFocused } from '@/lib/windowFocus'
import { useAppLaunchOverlay } from '@/hooks/useAppLaunchOverlay'
import { useOverlay } from '@/hooks/useOverlay'
import { usePermissionOverlay } from '@/hooks/usePermissionOverlay'
import { useTabAudio } from '@/hooks/useTabAudio'
import { useTabWebviews, type Bounds, type WebviewLayout } from '@/hooks/useTabWebviews'
import { useVideoPopout } from '@/hooks/useVideoPopout'

import { FOCUS_LAUNCHPAD_SEARCH_EVENT } from '@/components/NewTabPage'
import { PANE_GLIDE_MS, PaneView, type PaneSide } from '@/components/SplitPaneHost'

const KEYACTION_EVENT = 'wis-keyaction'
const FIND_FOCUS_EVENT = 'wis-overlay-find-focus'
const FIND_CLOSE_REQUEST_EVENT = 'wis-overlay-find-close'

const FIND_STRIP_HEIGHT_PX = 80

interface TabWebViewHostProps {
  state: BrowserState
  /** Reports the set of discarded ("asleep") tab ids when it changes. */
  onDiscardedChange?: (discarded: Set<TabId>) => void
}

export function TabWebViewHost({ state, onDiscardedChange }: TabWebViewHostProps) {
  const hostRef = useRef<HTMLDivElement | null>(null)
  usePermissionOverlay(hostRef)
  useAppLaunchOverlay(hostRef)
  const findOverlay = useOverlay('find')
  const findOpenRef = useRef(false)

  useEffect(() => {
    void (async () => {
      try {
        await commands.ensureOverlay('find')
        const el = hostRef.current
        const r = el?.getBoundingClientRect()
        if (r && r.width > 0) {
          await commands.sizeOverlay(
            'find',
            r.x,
            r.y + r.height - FIND_STRIP_HEIGHT_PX,
            r.width,
            FIND_STRIP_HEIGHT_PX,
          )
        }
      } catch {
        /* main window not ready; the lazy show path still works */
      }
    })()
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

  const tabsById = useMemo(() => {
    const map = new Map<TabId, Tab>()
    for (const t of workspaceTabs) map.set(t.id, t)
    return map
  }, [workspaceTabs])

  const paneModeKey = useMemo(
    () =>
      workspaceTabs
        .map((t) => (isChromeUrl(t.url) || t.load_error !== null ? '1' : '0'))
        .join(''),
    [workspaceTabs],
  )

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

  // A split adding a pane while others persist plays the chrome-side glide.
  const leaves = useMemo(() => leafIdsOf(effectiveTree), [effectiveTree])
  const prevLeavesRef = useRef<ReadonlySet<TabId>>(new Set())
  const prevTreeRef = useRef<PaneNode | null>(null)
  const prevLeaves = prevLeavesRef.current
  const paneDiff = useMemo(() => {
    const added = new Set<TabId>()
    const removed = new Set<TabId>()
    let persisting = false
    for (const id of leaves) {
      if (prevLeaves.has(id)) persisting = true
      else added.add(id)
    }
    for (const id of prevLeaves) {
      if (!leaves.has(id)) removed.add(id)
    }
    return { added, removed, persisting }
  }, [leaves, prevLeaves])
  const motionOff = document.body.classList.contains('motion-off')
  const newLeafIds =
    !motionOff && paneDiff.persisting && paneDiff.added.size > 0 && paneDiff.removed.size === 0
      ? paneDiff.added
      : EMPTY_LEAVES
  const [paneAnim, setPaneAnim] = useState(false)
  const paneAnimRef = useRef(false)
  if (newLeafIds.size > 0) paneAnimRef.current = true
  const paneAnimTimerRef = useRef<number | null>(null)
  // Removals (pane close, Cmd+Opt+U unsplit) keep painting the outgoing
  // tree while its closing slots glide shut, then commit the new tree.
  const [exitAnim, setExitAnim] = useState<{
    tree: PaneNode
    closing: ReadonlySet<TabId>
  } | null>(null)
  const wantExit =
    !motionOff &&
    paneDiff.persisting &&
    paneDiff.removed.size > 0 &&
    paneDiff.added.size === 0 &&
    prevTreeRef.current !== null &&
    prevTreeRef.current.kind === 'split'
  if (wantExit && exitAnim === null && prevTreeRef.current !== null) {
    setExitAnim({ tree: prevTreeRef.current, closing: paneDiff.removed })
    paneAnimRef.current = true
  }
  useLayoutEffect(() => {
    prevLeavesRef.current = leaves
    prevTreeRef.current = effectiveTree
  }, [leaves, effectiveTree])
  useLayoutEffect(() => {
    if (newLeafIds.size === 0 && exitAnim === null) return
    setPaneAnim(true)
    if (paneAnimTimerRef.current !== null) window.clearTimeout(paneAnimTimerRef.current)
    paneAnimTimerRef.current = window.setTimeout(() => {
      paneAnimTimerRef.current = null
      paneAnimRef.current = false
      setPaneAnim(false)
      setExitAnim(null)
    }, PANE_GLIDE_MS + 60)
  }, [newLeafIds, exitAnim])
  useEffect(
    () => () => {
      if (paneAnimTimerRef.current !== null) window.clearTimeout(paneAnimTimerRef.current)
    },
    [],
  )

  const createTabs = useMemo(
    () => workspaceTabs.filter((t) => !isChromeUrl(t.url)),
    [workspaceTabs],
  )

  // Tabs inside a collapsed folder: explicitly tucked away, so let them sleep.
  const collapsedFolderTabIds = useMemo(() => {
    const collapsed = new Set(
      (activeWorkspace?.tab_folders ?? []).filter((f) => f.collapsed).map((f) => f.id),
    )
    const ids = new Set<TabId>()
    if (collapsed.size === 0) return ids
    for (const t of state.tabs) {
      if (t.folder_id != null && collapsed.has(t.folder_id)) ids.add(t.id)
    }
    return ids
  }, [activeWorkspace?.tab_folders, state.tabs])

  const { controller: popout, poppedTabId } = useVideoPopout({
    enabled: state.settings.video_popout_enabled,
  })
  const tabAudio = useTabAudio()

  const { syncLayout } = useTabWebviews({
    createTabs,
    activeWorkspaceId: activeWorkspace?.id ?? null,
    allTabs: state.tabs,
    activeTabId: activeWorkspace?.active_tab_id ?? null,
    sleepEnabled: state.settings.tab_sleep_enabled,
    sleepMinutes: state.settings.tab_sleep_minutes,
    forceSleepTabIds: collapsedFolderTabIds,
    onDiscardedChange,
    audibleTabIds: tabAudio.audible,
    popout,
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

  // Manual video popout: pop the active tab's video out (or fold it back)
  // on the Pop Out Video key action.
  const focusableTabRef = useRef<TabId | null>(null)
  focusableTabRef.current = activeTab && !isChromeUrl(activeTab.url) ? activeTab.id : null
  useEffect(() => {
    let cancelled = false
    let unlisten: UnlistenFn | null = null
    void listen<{ action: KeyAction }>(KEYACTION_EVENT, (e) => {
      if (!isWindowFocused()) return
      if (e.payload.action !== 'pop_out_video') return
      const id = focusableTabRef.current
      if (id) popout.requestPopOut(id)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [popout])

  // A manually popped-out tab keeps its pane, which renders the black
  // player placeholder; clicking it folds the video back in.
  const poppedTabIds = useMemo<ReadonlySet<TabId>>(
    () => (poppedTabId ? new Set([poppedTabId]) : EMPTY_LEAVES),
    [poppedTabId],
  )
  const restorePopout = useCallback((id: TabId) => popout.release(id), [popout])

  // Measure each webview pane-leaf and tile its native webview into place.
  const focusWebviewId =
    activeTab && !isChromeUrl(activeTab.url) ? activeTab.id : null

  useEffect(() => {
    if (currentPlatform() === 'macos' || focusWebviewId !== null) return
    if (findOpenRef.current || document.hasFocus()) return
    void commands.focusSelf().catch(() => undefined)
  }, [focusWebviewId])

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

  useLayoutEffect(() => {
    const host = hostRef.current
    if (!host) return
    const measure = () => {
      if (resizingRef.current || draggingRef.current || paneAnimRef.current) {
        syncLayout(new Map<TabId, Bounds>(), null, true)
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

      if (findOpenRef.current) {
        const rect = findRect()
        if (rect) {
          void commands
            .sizeOverlay('find', rect.x, rect.y, rect.width, rect.height)
            .catch(() => undefined)
        }
      }
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
  }, [treeKey, paneModeKey, focusWebviewId, syncLayout, resizing, draggingTab, paneAnim, findRect])

  // Find-overlay orchestration: open on Cmd+F, hide when the surface requests
  // close, keep the strip pinned to the bottom on window resize.
  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []

    const openFind = async () => {
      const rect = findRect()
      if (!rect) return
      await findOverlay.show(rect)
      // Already-visible bar summoned again (Cmd+F after a peek opened over
      // it): the reveal-transition restack can't fire, so promote explicitly.
      await commands.promoteOverlay('find').catch(() => undefined)
      findOpenRef.current = true
      await emit(FIND_FOCUS_EVENT)
    }
    const closeFind = async () => {
      findOpenRef.current = false
      await findOverlay.hide()
      if (currentPlatform() !== 'macos') {
        const id = focusableTabRef.current
        if (id) void commands.focusWebview(`tab-${id}`).catch(() => undefined)
        else void commands.focusSelf().catch(() => undefined)
      }
    }

    void listen<{ action: KeyAction }>(KEYACTION_EVENT, (e) => {
      if (!isWindowFocused()) return
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
      window.dispatchEvent(new CustomEvent('wis-close-workspace-popover'))
      const id = e.payload as TabId
      if (id === activeIdRef.current) return
      if (popout.isPopout(id)) return
      void commands.setActiveTab(id).catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [popout])

  return (
    <div ref={hostRef} className="kb-content-host">
      {effectiveTree ? (
        <PaneView
          node={exitAnim?.tree ?? effectiveTree}
          path={[]}
          tabsById={tabsById}
          focusedId={activeWorkspace?.active_tab_id ?? null}
          onFocusPane={focusPane}
          onRatioCommit={commitRatio}
          dragActive={!!draggingTab}
          movedTabId={draggingTab}
          onPaneDrop={handlePaneDrop}
          newLeafIds={newLeafIds}
          closingLeafIds={exitAnim?.closing ?? EMPTY_LEAVES}
          poppedTabIds={poppedTabIds}
          onRestorePopout={restorePopout}
        />
      ) : null}
    </div>
  )
}

const EMPTY_LEAVES: ReadonlySet<TabId> = new Set()

/** Collect the tab ids of every leaf in a pane tree. */
function leafIdsOf(node: PaneNode | null): ReadonlySet<TabId> {
  const out = new Set<TabId>()
  const walk = (n: PaneNode | null): void => {
    if (!n) return
    if (n.kind === 'leaf') out.add(n.tab)
    else {
      walk(n.first)
      walk(n.second)
    }
  }
  walk(node)
  return out
}
