import { useCallback, useEffect, useRef } from 'react'
import { LogicalPosition, LogicalSize } from '@tauri-apps/api/dpi'
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

interface Bounds {
  x: number
  y: number
  width: number
  height: number
}

interface UseTabWebviewsArgs {
  tabs: Tab[]
  activeTabId: TabId | null
  activeWorkspaceId: WorkspaceId | null
}

interface UseTabWebviewsReturn {
  /** Forward the content area's bounds to the active webview. */
  setBounds: (bounds: Bounds) => void
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
  tabs,
  activeTabId,
  activeWorkspaceId,
}: UseTabWebviewsArgs): UseTabWebviewsReturn {
  const webviewsRef = useRef<Map<TabId, Webview>>(new Map())
  const boundsRef = useRef<Bounds | null>(null)
  const windowRef = useRef<Window | null>(null)
  const activeTabIdRef = useRef<TabId | null>(activeTabId)
  activeTabIdRef.current = activeTabId
  const visibilityRef = useRef<Map<TabId, boolean>>(new Map())
  const rafRef = useRef<number | null>(null)
  const pendingBoundsRef = useRef<Bounds | null>(null)
  const lastAppliedRef = useRef<{ id: TabId; bounds: Bounds } | null>(null)

  if (windowRef.current === null) {
    windowRef.current = getCurrentWindow()
  }

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

  // Reconcile webview set against target tabs.
  useEffect(() => {
    if (!activeWorkspaceId || !windowRef.current) return
    const window = windowRef.current
    const map = webviewsRef.current
    const targetTabs = tabs.filter((t) => !isChromeUrl(t.url))
    const targetIds = new Set(targetTabs.map((t) => t.id))

    // Destroy any webviews whose tab no longer wants one.
    for (const [id, wv] of Array.from(map.entries())) {
      if (!targetIds.has(id)) {
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

    // Create webviews for tabs that don't have one yet.
    const initialSize = boundsRef.current
      ? { width: boundsRef.current.width, height: boundsRef.current.height }
      : null
    for (const tab of targetTabs) {
      if (map.has(tab.id)) continue
      map.set(tab.id, PENDING as unknown as Webview)

      void createWebviewFor(window, tab, activeWorkspaceId, initialSize).then(
        (wv) => {
          if (!targetIds.has(tab.id)) {
            map.delete(tab.id)
            void wv.close().catch(() => undefined)
            return
          }
          map.set(tab.id, wv)
          void commands
            .setWebviewCornerRadius(labelFor(tab.id), WEBVIEW_CORNER_RADIUS)
            .catch(() => undefined)
          if (tab.id === activeTabIdRef.current) {
            applyBounds(wv, boundsRef.current)
            void wv.show().catch(() => undefined)
            void wv.setFocus().catch(() => undefined)
            visibilityRef.current.set(tab.id, true)
          } else {
            void wv.hide().catch(() => undefined)
            visibilityRef.current.set(tab.id, false)
          }
        },
        (err: unknown) => {
          map.delete(tab.id)
          // eslint-disable-next-line no-console
          console.error(`create webview ${tab.id}`, err)
        },
      )
    }
  }, [tabs, activeWorkspaceId])

  useEffect(() => {
    const entries = Array.from(webviewsRef.current.entries())
    const visibility = visibilityRef.current

    for (const [id, wv] of entries) {
      if ((wv as unknown) === PENDING) continue
      if (id !== activeTabId && visibility.get(id) !== false) {
        void wv.hide().catch(() => undefined)
        visibility.set(id, false)
      }
    }
    for (const [id, wv] of entries) {
      if ((wv as unknown) === PENDING) continue
      if (id === activeTabId && visibility.get(id) !== true) {
        applyBounds(wv, boundsRef.current)
        void wv.show().catch(() => undefined)
        void wv.setFocus().catch(() => undefined)
        visibility.set(id, true)
      }
    }
  }, [activeTabId, tabs])

  const flushBounds = useCallback(() => {
    rafRef.current = null
    const bounds = pendingBoundsRef.current
    const id = activeTabIdRef.current
    if (!bounds || !id) return
    const wv = webviewsRef.current.get(id)
    if (!wv || (wv as unknown) === PENDING) return

    // Drop no-op updates
    const last = lastAppliedRef.current
    if (last && last.id === id && boundsEqual(last.bounds, bounds)) return
    lastAppliedRef.current = { id, bounds }
    applyBounds(wv, bounds)
  }, [])

  const setBounds = useCallback(
    (bounds: Bounds) => {
      boundsRef.current = bounds
      pendingBoundsRef.current = bounds
      if (rafRef.current === null) {
        rafRef.current = requestAnimationFrame(flushBounds)
      }
    },
    [flushBounds],
  )

  useEffect(() => {
    return () => {
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current)
    }
  }, [])

  return { setBounds }
}

function boundsEqual(a: Bounds, b: Bounds): boolean {
  return (
    Math.abs(a.x - b.x) < 0.5 &&
    Math.abs(a.y - b.y) < 0.5 &&
    Math.abs(a.width - b.width) < 0.5 &&
    Math.abs(a.height - b.height) < 0.5
  )
}

function applyBounds(wv: Webview, bounds: Bounds | null): void {
  if (!bounds) return
  void wv
    .setPosition(new LogicalPosition(bounds.x, bounds.y))
    .catch(() => undefined)
  void wv.setSize(new LogicalSize(bounds.width, bounds.height)).catch(() => undefined)
}
