import { useCallback, useEffect, useMemo, useRef } from 'react'
import { Webview } from '@tauri-apps/api/webview'
import { Window, getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { Tab, TabId, WorkspaceId } from '@/bindings/types'
import { isChromeUrl } from '@/lib/url'
import type { VideoPopoutController } from '@/hooks/useVideoPopout'

const WEBVIEW_CORNER_RADIUS = 12

const LABEL_PREFIX = 'tab-'
const PENDING = Symbol('pending-webview')
const OFFSCREEN_BOUNDS: Bounds = { x: 100000, y: 100000, width: 800, height: 600 }
const PLACEHOLDER_BOUNDS: Bounds = OFFSCREEN_BOUNDS

// How often the idle sweep checks for background webviews to discard.
const SWEEP_INTERVAL_MS = 60_000

// Most recently slept tabs whose serialized session (WKWebView
// interactionState: history, scroll, forms) is kept for restore-on-wake.
// Each blob can be tens of KB, so the cache is bounded; evicted tabs just
// cold-load like before.
const MAX_SLEEP_STATES = 30

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
  /** Non-chrome tabs of the active workspace; the pool a visible tab is drawn from. */
  createTabs: Tab[]
  activeWorkspaceId: WorkspaceId | null
  /**
   * Every tab across all workspaces. A webview is only destroyed by close when
   * its tab disappears from this set (a real close). Background webviews may
   * still be discarded by the idle sweep and recreated on next focus.
   */
  allTabs: Tab[]
  /** The active tab; never discarded by the sleep sweep. */
  activeTabId: TabId | null
  /** When true, background webviews idle past `sleepMinutes` are discarded. */
  sleepEnabled: boolean
  sleepMinutes: number
  /**
   * Tabs in collapsed folders: discarded immediately (not after the idle delay)
   * while sleeping is enabled, since a collapsed folder is explicitly tucked away.
   */
  forceSleepTabIds?: Set<TabId>
  /** Reports the current set of discarded ("asleep") tab ids when it changes. */
  onDiscardedChange?: (discarded: Set<TabId>) => void
  /**
   * Tabs audibly playing right now: never discarded by any sleep path.
   * They sleep normally once the sound stops.
   */
  audibleTabIds?: ReadonlySet<TabId>
  /**
   * Video-popout orchestration: a hidden tab playing video floats as the
   * native mini player instead of just hiding, and folds back inline when
   * it rejoins the layout. A popped-out tab is natively visible, so it is
   * exempt from hiding and from every sleep path.
   */
  popout?: VideoPopoutController
}

interface UseTabWebviewsReturn {
  /**
   * Reconcile native webview visibility/position to `layout` and move native
   * focus to `focusedTabId`. Called directly from the host's measurement effects.
   * Webviews for newly-visible tabs are created lazily here. `transient`
   * marks a momentary hide-all (divider drag, pane animation) that must not
   * trigger popout adoption.
   */
  syncLayout: (layout: WebviewLayout, focusedTabId: TabId | null, transient?: boolean) => void
}

function labelFor(tabId: TabId): string {
  return `${LABEL_PREFIX}${tabId}`
}

async function createWebviewFor(
  window: Window,
  tab: Tab,
  workspaceId: WorkspaceId,
  initialSize: { width: number; height: number } | null,
  restoreState: string | null,
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
    restoreState,
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
  activeTabId,
  sleepEnabled,
  sleepMinutes,
  forceSleepTabIds,
  onDiscardedChange,
  audibleTabIds,
  popout,
}: UseTabWebviewsArgs): UseTabWebviewsReturn {
  const webviewsRef = useRef<Map<TabId, Webview>>(new Map())
  const windowRef = useRef<Window | null>(null)
  const visibilityRef = useRef<Map<TabId, boolean>>(new Map())
  // Latest desired layout + focus.
  const layoutRef = useRef<WebviewLayout>(new Map())
  const focusedRef = useRef<TabId | null>(null)
  const lastFocusAppliedRef = useRef<TabId | null>(null)
  // When each currently-hidden webview last went hidden (for the sleep sweep).
  const hiddenSinceRef = useRef<Map<TabId, number>>(new Map())
  // Tabs whose webview has been discarded ("asleep").
  const discardedRef = useRef<Set<TabId>>(new Set())
  // Serialized sessions of slept tabs, consumed by the next wake.
  const sleepStatesRef = useRef<Map<TabId, string>>(new Map())

  // Mirror props into refs so the lazily-invoked callbacks stay current without
  // re-subscribing the measurement effects.
  const createTabsRef = useRef(createTabs)
  createTabsRef.current = createTabs
  const allTabsRef = useRef(allTabs)
  allTabsRef.current = allTabs
  const activeWorkspaceIdRef = useRef(activeWorkspaceId)
  activeWorkspaceIdRef.current = activeWorkspaceId
  const activeTabIdRef = useRef(activeTabId)
  activeTabIdRef.current = activeTabId
  const sleepEnabledRef = useRef(sleepEnabled)
  sleepEnabledRef.current = sleepEnabled
  const sleepMinutesRef = useRef(sleepMinutes)
  sleepMinutesRef.current = sleepMinutes
  const forceSleepRef = useRef<Set<TabId>>(forceSleepTabIds ?? new Set())
  forceSleepRef.current = forceSleepTabIds ?? new Set()
  const onDiscardedRef = useRef(onDiscardedChange)
  onDiscardedRef.current = onDiscardedChange
  const audibleRef = useRef<ReadonlySet<TabId>>(audibleTabIds ?? new Set())
  audibleRef.current = audibleTabIds ?? new Set()
  const popoutRef = useRef(popout)
  popoutRef.current = popout
  // True while the current layout is a momentary hide-all.
  const transientRef = useRef(false)

  if (windowRef.current === null) {
    windowRef.current = getCurrentWindow()
  }

  // Recompute the "asleep" set: every non-chrome tab that has no live webview
  // (never loaded yet after a restart, or discarded by the sweep). Notifies the
  // consumer only when the set actually changes.
  const recomputeDiscarded = useCallback(() => {
    const map = webviewsRef.current
    const next = new Set<TabId>()
    for (const t of allTabsRef.current) {
      if (isChromeUrl(t.url)) continue
      const wv = map.get(t.id)
      const loaded = wv != null && (wv as unknown) !== PENDING
      if (!loaded) next.add(t.id)
    }
    const prev = discardedRef.current
    let changed = next.size !== prev.size
    if (!changed) {
      for (const id of next) {
        if (!prev.has(id)) {
          changed = true
          break
        }
      }
    }
    if (changed) {
      discardedRef.current = next
      onDiscardedRef.current?.(new Set(next))
    }
  }, [])

  // Discard a background webview, first capturing its serialized session so
  // the next wake restores history/scroll/forms instead of cold-loading.
  // Set once `ensureWebview` exists; lets the sleep teardown revive a tab
  // that rejoined the layout while its old webview was still closing.
  const ensureWebviewRef = useRef<((id: TabId) => void) | null>(null)

  const sleepWebview = useCallback(
    (id: TabId, wv: Webview) => {
      // Hold the slot with pending until the native close completes.
      webviewsRef.current.set(id, PENDING as unknown as Webview)
      void commands
        .tabInteractionState(id)
        .then((state) => {
          if (!state) return
          const cache = sleepStatesRef.current
          cache.delete(id)
          cache.set(id, state)
          while (cache.size > MAX_SLEEP_STATES) {
            const oldest = cache.keys().next().value
            if (oldest === undefined) break
            cache.delete(oldest)
          }
        })
        .catch(() => undefined)
        .finally(() => {
          void wv
            .close()
            .catch((err: unknown) => {
              // eslint-disable-next-line no-console
              console.error(`sleep close ${id}`, err)
            })
            .finally(() => {
              if ((webviewsRef.current.get(id) as unknown) === PENDING) {
                webviewsRef.current.delete(id)
              }
              recomputeDiscarded()
              if (layoutRef.current.has(id)) ensureWebviewRef.current?.(id)
            })
        })
    },
    [recomputeDiscarded],
  )

  // Reconcile a single webview's visibility/position against the desired layout.
  const reconcileOne = useCallback((id: TabId, wv: Webview) => {
    const rect = layoutRef.current.get(id)
    const vis = visibilityRef.current
    if (rect) {
      if (popoutRef.current?.isPopout(id)) {
        popoutRef.current.onVisible(id)
        return
      }
      hiddenSinceRef.current.delete(id)
      if (vis.get(id) !== true) {
        vis.set(id, true)
        showWithBounds(wv, rect)
      } else {
        applyBounds(wv, rect)
      }
    } else {
      // The popout owns this webview's visibility and bounds while it floats.
      if (popoutRef.current?.isPopout(id)) return
      if (!hiddenSinceRef.current.has(id)) {
        hiddenSinceRef.current.set(id, Date.now())
      }
      if (vis.get(id) !== false) {
        vis.set(id, false)
        void wv.hide().catch(() => undefined)
        if (!transientRef.current && id !== activeTabIdRef.current) {
          popoutRef.current?.onHidden(id)
        }
      }
    }
  }, [])

  // Create the webview for a tab that has become visible but has none yet.
  const ensureWebview = useCallback(
    (tabId: TabId) => {
      const map = webviewsRef.current
      const window = windowRef.current
      const wsId = activeWorkspaceIdRef.current
      if (!window || !wsId || map.has(tabId)) return
      const tab = createTabsRef.current.find((t) => t.id === tabId)
      if (!tab) return

      map.set(tabId, PENDING as unknown as Webview)
      const rect = layoutRef.current.get(tabId)
      const initialSize = rect ? { width: rect.width, height: rect.height } : null
      const restoreState = sleepStatesRef.current.get(tabId) ?? null
      sleepStatesRef.current.delete(tabId)

      void createWebviewFor(window, tab, wsId, initialSize, restoreState).then(
        (wv) => {
          const stillLive = allTabsRef.current.some(
            (t) => t.id === tabId && !isChromeUrl(t.url),
          )
          if (!stillLive) {
            map.delete(tabId)
            void wv.close().catch(() => undefined)
            return
          }
          map.set(tabId, wv)
          recomputeDiscarded()
          void commands
            .setWebviewCornerRadius(labelFor(tabId), WEBVIEW_CORNER_RADIUS)
            .catch(() => undefined)
          reconcileOne(tabId, wv)
          if (tabId === focusedRef.current) {
            void wv.setFocus().catch(() => undefined)
            lastFocusAppliedRef.current = tabId
          }
        },
        (err: unknown) => {
          map.delete(tabId)
          // eslint-disable-next-line no-console
          console.error(`create webview ${tabId}`, err)
        },
      )
    },
    [reconcileOne, recomputeDiscarded],
  )
  ensureWebviewRef.current = ensureWebview

  // Close any leftover webviews from a previous run on first mount.
  useEffect(() => {
    let cancelled = false
    void Webview.getAll().then(async (all) => {
      if (cancelled) return
      const myWindow = windowRef.current?.label
      const liveLabels = new Set(
        Array.from(webviewsRef.current.keys()).map((id) => labelFor(id)),
      )
      for (const wv of all) {
        if (!wv.label.startsWith(LABEL_PREFIX)) continue
        if (wv.window.label !== myWindow) continue
        if (liveLabels.has(wv.label)) continue
        await wv.close().catch(() => undefined)
      }
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Destroy webviews whose tab was actually closed (gone from every workspace).
  // Also refreshes the "asleep" set so newly-added tabs dim until first shown
  // and closed tabs drop out of it.
  useEffect(() => {
    if (!activeWorkspaceId) return
    const map = webviewsRef.current
    const liveIds = new Set(allTabs.filter((t) => !isChromeUrl(t.url)).map((t) => t.id))

    for (const [id, wv] of Array.from(map.entries())) {
      if (liveIds.has(id)) continue
      // A popped-out tab folds back natively before its webview dies.
      popoutRef.current?.noteClosed(id)
      if ((wv as unknown) !== PENDING) {
        void wv.close().catch((err: unknown) => {
          // eslint-disable-next-line no-console
          console.error(`close webview ${id}`, err)
        })
      }
      map.delete(id)
      visibilityRef.current.delete(id)
      hiddenSinceRef.current.delete(id)
      sleepStatesRef.current.delete(id)
    }
    recomputeDiscarded()
  }, [allTabs, activeWorkspaceId, recomputeDiscarded])

  // Idle sweep: discard background webviews hidden past the threshold.
  useEffect(() => {
    const interval = setInterval(() => {
      if (!sleepEnabledRef.current) return
      const map = webviewsRef.current
      const now = Date.now()
      const thresholdMs = Math.max(1, sleepMinutesRef.current) * 60_000
      let changed = false
      for (const [id, wv] of Array.from(map.entries())) {
        if ((wv as unknown) === PENDING) continue
        if (layoutRef.current.has(id)) continue // visible
        if (id === activeTabIdRef.current) continue
        if (popoutRef.current?.isPopout(id)) continue // floating mini player
        if (audibleRef.current.has(id)) continue // audibly playing
        const tab = allTabsRef.current.find((t) => t.id === id)
        if (tab?.pinned) continue
        // Collapsed-folder tabs skip the idle delay.
        if (!forceSleepRef.current.has(id)) {
          const since = hiddenSinceRef.current.get(id)
          if (since == null) {
            hiddenSinceRef.current.set(id, now)
            continue
          }
          if (now - since < thresholdMs) continue
        }
        sleepWebview(id, wv)
        visibilityRef.current.delete(id)
        hiddenSinceRef.current.delete(id)
        changed = true
      }
      if (changed) recomputeDiscarded()
    }, SWEEP_INTERVAL_MS)
    return () => clearInterval(interval)
  }, [recomputeDiscarded, sleepWebview])

  // Discard collapsed-folder tabs the moment they're collapsed, rather than
  // waiting for the next sweep. The key collapses the set to a stable string so
  // this only fires when membership actually changes. Audibility changes also
  // re-run it: a playing tab is exempt while it sounds, and must be reaped as
  // soon as it goes silent (pause, mute, media end), not a sweep-tick later.
  const forceSleepKey = useMemo(
    () => Array.from(forceSleepTabIds ?? []).sort().join(','),
    [forceSleepTabIds],
  )
  const audibleKey = useMemo(
    () => Array.from(audibleTabIds ?? []).sort().join(','),
    [audibleTabIds],
  )
  useEffect(() => {
    if (!sleepEnabledRef.current) return
    const map = webviewsRef.current
    let changed = false
    for (const id of forceSleepRef.current) {
      if (id === activeTabIdRef.current) continue
      if (layoutRef.current.has(id)) continue
      if (popoutRef.current?.isPopout(id)) continue
      if (audibleRef.current.has(id)) continue // audibly playing
      const wv = map.get(id)
      if (wv == null || (wv as unknown) === PENDING) continue
      sleepWebview(id, wv)
      visibilityRef.current.delete(id)
      hiddenSinceRef.current.delete(id)
      changed = true
    }
    if (changed) recomputeDiscarded()
  }, [forceSleepKey, audibleKey, recomputeDiscarded, sleepWebview])

  // After a popout restore lands, the webview is hidden in the main window.
  useEffect(() => {
    popout?.setReconcile((id: TabId) => {
      const wv = webviewsRef.current.get(id)
      if (!wv || (wv as unknown) === PENDING) return
      hiddenSinceRef.current.delete(id)
      visibilityRef.current.set(id, false)
      reconcileOne(id, wv)
    })
  }, [popout, reconcileOne])

  const syncLayout = useCallback(
    (layout: WebviewLayout, focusedTabId: TabId | null, transient = false) => {
      layoutRef.current = layout
      focusedRef.current = focusedTabId
      transientRef.current = transient
      // Lazily create webviews for tabs that just became visible.
      for (const id of layout.keys()) {
        if (!webviewsRef.current.has(id)) ensureWebview(id)
      }
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
    [reconcileOne, ensureWebview],
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

