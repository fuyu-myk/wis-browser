import { useCallback, useEffect, useRef } from 'react'
import { Webview } from '@tauri-apps/api/webview'
import { Window, getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { Tab, TabId, WorkspaceId } from '@/bindings/types'
import { isChromeUrl } from '@/lib/url'

const WEBVIEW_CORNER_RADIUS = 12

const LABEL_PREFIX = 'tab-'
const PENDING = Symbol('pending-webview')
const OFFSCREEN_BOUNDS: Bounds = { x: 100000, y: 100000, width: 800, height: 600 }
const PLACEHOLDER_BOUNDS: Bounds = OFFSCREEN_BOUNDS

export interface Bounds {
  x: number
  y: number
  width: number
  height: number
}

/**
 * The set of webview tabs that should be visible right now, each mapped to the
 * pane rect it should fill. Tabs not present are hidden. In single-pane mode
 * this has one entry (the active tab); in split mode one per visible pane leaf.
 */
export type WebviewLayout = Map<TabId, Bounds>

interface UseTabWebviewsArgs {
  /** Non-chrome tabs of the active workspace to drive webview creation. */
  createTabs: Tab[]
  activeWorkspaceId: WorkspaceId | null
  /**
   * Every tab across all workspaces. A webview is only destroyed when its tab
   * disappears from this set (a real close); switching workspaces keeps the
   * others' webviews alive (hidden) so their pages don't reload.
   */
  allTabs: Tab[]
}

interface UseTabWebviewsReturn {
  /**
   * Reconcile native webview visibility/position to `layout` and move native
   * focus to `focusedTabId`. Called directly from the host's measurement effects.
   */
  syncLayout: (layout: WebviewLayout, focusedTabId: TabId | null) => void
}

function labelFor(tabId: TabId): string {
  return `${LABEL_PREFIX}${tabId}`
}

async function createWebviewFor(
  window: Window,
  tab: Tab,
  workspaceId: WorkspaceId,
  initialSize: { width: number; height: number } | null,
): Promise<Webview> {
  const label = labelFor(tab.id)
  const width = initialSize?.width ?? PLACEHOLDER_BOUNDS.width
  const height = initialSize?.height ?? PLACEHOLDER_BOUNDS.height
  await commands.createTabWebview({
    windowLabel: window.label,
    label,
    url: tab.url,
    workspaceId,
    tabId: tab.id,
    x: PLACEHOLDER_BOUNDS.x,
    y: PLACEHOLDER_BOUNDS.y,
    width,
    height,
  })

  const wv = await Webview.getByLabel(label)
  if (!wv) {
    throw new Error(`webview ${label} created but not retrievable`)
  }
  return wv
}

export function useTabWebviews({
  createTabs,
  activeWorkspaceId,
  allTabs,
}: UseTabWebviewsArgs): UseTabWebviewsReturn {
  const webviewsRef = useRef<Map<TabId, Webview>>(new Map())
  const windowRef = useRef<Window | null>(null)
  const visibilityRef = useRef<Map<TabId, boolean>>(new Map())
  // Latest desired layout + focus.
  const layoutRef = useRef<WebviewLayout>(new Map())
  const focusedRef = useRef<TabId | null>(null)
  const lastFocusAppliedRef = useRef<TabId | null>(null)

  if (windowRef.current === null) {
    windowRef.current = getCurrentWindow()
  }

  // Reconcile a single webview's visibility/position against the desired layout.
  const reconcileOne = useCallback((id: TabId, wv: Webview) => {
    const rect = layoutRef.current.get(id)
    const vis = visibilityRef.current
    if (rect) {
      if (vis.get(id) !== true) {
        vis.set(id, true)
        showWithBounds(wv, rect)
      } else {
        applyBounds(wv, rect)
      }
    } else if (vis.get(id) !== false) {
      vis.set(id, false)
      void wv.hide().catch(() => undefined)
    }
  }, [])

  // Close any leftover webviews from a previous run on first mount.
  useEffect(() => {
    let cancelled = false
    void Webview.getAll().then(async (all) => {
      if (cancelled) return
      const liveLabels = new Set(
        Array.from(webviewsRef.current.keys()).map((id) => labelFor(id)),
      )
      for (const wv of all) {
        if (!wv.label.startsWith(LABEL_PREFIX)) continue
        if (liveLabels.has(wv.label)) continue
        await wv.close().catch(() => undefined)
      }
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Create/destroy webviews to match the live tab set.
  useEffect(() => {
    if (!activeWorkspaceId || !windowRef.current) return
    const window = windowRef.current
    const map = webviewsRef.current
    const liveIds = new Set(
      allTabs.filter((t) => !isChromeUrl(t.url)).map((t) => t.id),
    )

    // Destroy only webviews whose tab was actually closed (gone everywhere).
    for (const [id, wv] of Array.from(map.entries())) {
      if (!liveIds.has(id)) {
        if ((wv as unknown) !== PENDING) {
          void wv.close().catch((err: unknown) => {
            // eslint-disable-next-line no-console
            console.error(`close webview ${id}`, err)
          })
        }
        map.delete(id)
        visibilityRef.current.delete(id)
      }
    }

    // Create webviews for active-workspace tabs that don't have one yet.
    const firstRect = layoutRef.current.values().next().value as Bounds | undefined
    const initialSize = firstRect
      ? { width: firstRect.width, height: firstRect.height }
      : null
    for (const tab of createTabs) {
      if (map.has(tab.id)) continue
      map.set(tab.id, PENDING as unknown as Webview)

      void createWebviewFor(window, tab, activeWorkspaceId, initialSize).then(
        (wv) => {
          if (!liveIds.has(tab.id)) {
            map.delete(tab.id)
            void wv.close().catch(() => undefined)
            return
          }
          map.set(tab.id, wv)
          void commands
            .setWebviewCornerRadius(labelFor(tab.id), WEBVIEW_CORNER_RADIUS)
            .catch(() => undefined)
          reconcileOne(tab.id, wv)
          if (tab.id === focusedRef.current) {
            void wv.setFocus().catch(() => undefined)
            lastFocusAppliedRef.current = tab.id
          }
        },
        (err: unknown) => {
          map.delete(tab.id)
          // eslint-disable-next-line no-console
          console.error(`create webview ${tab.id}`, err)
        },
      )
    }
  }, [createTabs, allTabs, activeWorkspaceId, reconcileOne])

  const syncLayout = useCallback(
    (layout: WebviewLayout, focusedTabId: TabId | null) => {
      layoutRef.current = layout
      focusedRef.current = focusedTabId
      for (const [id, wv] of webviewsRef.current) {
        if ((wv as unknown) === PENDING) continue
        reconcileOne(id, wv)
      }
      if (focusedTabId && focusedTabId !== lastFocusAppliedRef.current) {
        const wv = webviewsRef.current.get(focusedTabId)
        if (wv && (wv as unknown) !== PENDING) {
          void wv.setFocus().catch(() => undefined)
          lastFocusAppliedRef.current = focusedTabId
        }
      }
    },
    [reconcileOne],
  )

  return { syncLayout }
}

/**
 * Reveal a (possibly previously-hidden) webview and size it to `bounds`.
 */
function showWithBounds(wv: Webview, bounds: Bounds): void {
  applyBounds(wv, bounds)
  void wv
    .show()
    .then(() => applyBounds(wv, bounds))
    .catch(() => undefined)
}

function applyBounds(wv: Webview, bounds: Bounds): void {
  void commands
    .setTabWebviewBounds(wv.label, bounds.x, bounds.y, bounds.width, bounds.height)
    .catch(() => undefined)
}
