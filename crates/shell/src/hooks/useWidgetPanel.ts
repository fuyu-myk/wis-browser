import { useCallback, useEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { Widget, WidgetId } from '@/bindings/types'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useOverlay } from '@/hooks/useOverlay'
import { DEFAULT_PANEL_LIMIT, effectiveWidgetUrl } from '@/lib/widgets'
import {
  WPANEL_MUTE_EVENT,
  WPANEL_NAV_EVENT,
  WPANEL_RELOAD_EVENT,
  WPANEL_STATE_EVENT,
  WRESIZE_END_EVENT,
  WRESIZE_MOVE_EVENT,
  WRESIZE_START_EVENT,
  type WPanelNavPayload,
} from '@/lib/widgetResize'

/** Native event fired when a click lands inside any tab webview. */
const PANE_FOCUS_EVENT = 'wis-pane-focus'

const MIN_PANEL_WIDTH = 300
const MAX_PANEL_WIDTH = 680
const WIDTH_STORAGE_KEY = 'wis-widget-panel-width'
/**
 * Width of the invisible resize strip straddling the panel's inner edge. It
 * spans the panel's full height: like the window's own resize edges, the
 * affordance is the cursor change, not a visible handle.
 */
const GRIP_WIDTH = 14
/** Height of the accent header bar docked above the panel (reload button). */
const HEADER_HEIGHT = 30
/** Duration of the panel's slide-in entrance. */
const PANEL_ANIM_MS = 220

/** Slide distance from the dock edge. Capped at 48px to exactly match the
 * backdrop's CSS slide (--wpanel-slide), so the panel keeps covering it. */
function panelSlideDistance(panelWidth: number): number {
  return Math.min(48, Math.round(panelWidth * 0.15))
}

interface Rect {
  x: number
  y: number
  width: number
  height: number
}

function clampWidth(w: number): number {
  const dynamicMax = Math.min(MAX_PANEL_WIDTH, Math.round(window.innerWidth * 0.55))
  return Math.min(dynamicMax, Math.max(MIN_PANEL_WIDTH, Math.round(w)))
}

function storedWidth(): number {
  try {
    const raw = Number(localStorage.getItem(WIDTH_STORAGE_KEY))
    if (Number.isFinite(raw) && raw > 0) return clampWidth(raw)
  } catch {
    /* ignore */
  }
  return 400
}

/**
 * Floating panel geometry on the rail-nearest side of the content area: the
 * header bar on top, the panel webview below it, and the resize grip strip
 * spanning the panel's inner edge. Null when the content host isn't mounted.
 */
function computeGeometry(
  side: 'left' | 'right',
  width: number,
): { panel: Rect; grip: Rect; header: Rect } | null {
  const host = document.querySelector('.kb-content-host')
  if (!host) return null
  const r = host.getBoundingClientRect()
  const top = Math.round(r.top + 8)
  const x = side === 'left' ? Math.round(r.left + 8) : Math.round(r.right - width - 8)
  const y = top + HEADER_HEIGHT
  const height = Math.round(r.height - 16) - HEADER_HEIGHT
  const innerEdge = side === 'left' ? x + width : x
  return {
    header: { x, y: top, width, height: HEADER_HEIGHT },
    panel: { x, y, width, height },
    grip: { x: Math.round(innerEdge - GRIP_WIDTH / 2), y, width: GRIP_WIDTH, height },
  }
}

/**
 * Owns the quick-launch widget panels: per-widget webviews floated above the
 * tab webviews on the rail-nearest side. Clicking a widget toggles its panel;
 * switching widgets hides one panel and shows the other, so the change is
 * instant and every widget keeps its own in-page state (drafts, scroll, live
 * connections). The pool is bounded by the user's panel limit
 * (`widget_panel_limit_enabled` / `widget_panel_limit`, LRU-evicting hidden
 * panels; toggled off means limitless), and a panel hidden past the
 * tab-sleep idle window is destroyed.
 *
 * Resizing: native webviews swallow pointer events, so the drag handle is the
 * `overlay-widgetresize` surface (overlays are promoted above every webview),
 * an invisible strip on the visible panel's inner edge. The surface emits
 * drag deltas; this hook resizes the panel live, keeping its outer edge
 * anchored, and persists the shared width in localStorage.
 *
 * Guardrails: panels are bound to one workspace's data store + proxy, so all
 * are hard-destroyed on workspace switch; private workspaces destroy on
 * dismissal instead of hiding.
 */
export function useWidgetPanel(): {
  activeWidgetId: WidgetId | null
  toggleWidget: (widget: Widget) => void
  panelRect: { x: number; y: number; width: number; height: number } | null
  /** Dock side the panel grows out of (drives the backdrop grow origin). */
  panelSide: 'left' | 'right' 
} {
  const { data: state } = useBrowserState()
  const [activeWidgetId, setActiveWidgetId] = useState<WidgetId | null>(null)
  const [panelRect, setPanelRect] = useState<Rect | null>(null)
  const visibleRef = useRef<WidgetId | null>(null)
  visibleRef.current = activeWidgetId

  // Latest committed URL per panel (from the native page-load report), so the
  // header can show the visible widget's current host immediately on show.
  const panelUrlsRef = useRef<Map<WidgetId, string>>(new Map())

  // Push the visible widget's state (mute + current URL) to the header surface
  // so its toggle and host render correctly on every show.
  const pushHeaderState = useCallback((id: WidgetId) => {
    void commands
      .widgetPanelMuted(id)
      .then((muted) =>
        emit(WPANEL_STATE_EVENT, { muted, url: panelUrlsRef.current.get(id) ?? '' }),
      )
      .catch(() => undefined)
  }, [])

  useEffect(() => {
    if (activeWidgetId === null) return
    pushHeaderState(activeWidgetId)
  }, [activeWidgetId, pushHeaderState])
  /** Live panel webviews by widget id -> last-used timestamp (for LRU). */
  const liveRef = useRef<Map<WidgetId, number>>(new Map())
  /** Idle-destroy timers for hidden panels. */
  const timersRef = useRef<Map<WidgetId, number>>(new Map())
  const widthRef = useRef(storedWidth())
  const { show: showGrip, hide: hideGripOverlay } = useOverlay('widgetresize')
  const { show: showHeader, hide: hideHeaderOverlay } = useOverlay('widgetheader')
  /** Geometry snapshot taken when a grip drag starts. */
  const resizeBaseRef = useRef<{ startW: number; outerEdge: number; y: number; height: number } | null>(null)

  const layout = state?.settings.layout
  const side: 'left' | 'right' =
    layout === 'sidebar_right' || (layout === 'top_tabs' && state?.settings.top_tabs_rail_side === 'right')
      ? 'right'
      : 'left'
  const sideRef = useRef(side)
  sideRef.current = side

  const wsId = state?.active_workspace_id ?? null
  const ephemeral = state?.workspaces.find((w) => w.id === wsId)?.ephemeral ?? false
  const keepLoaded = state?.settings.widget_keep_loaded ?? true
  const sleepMinutes = state?.settings.widget_sleep_minutes ?? 5
  const sleepRef = useRef({ keepLoaded, minutes: sleepMinutes })
  sleepRef.current = { keepLoaded, minutes: sleepMinutes }
  const memCapEnabled = state?.settings.widget_memory_cap_enabled ?? true
  const memCapMb = state?.settings.widget_memory_cap_mb ?? 500
  const memCapMbRef = useRef(memCapMb)
  memCapMbRef.current = memCapMb
  const limitEnabled = state?.settings.widget_panel_limit_enabled ?? true
  const limitCount = state?.settings.widget_panel_limit ?? DEFAULT_PANEL_LIMIT
  const limitRef = useRef({ enabled: limitEnabled, count: limitCount })
  limitRef.current = { enabled: limitEnabled, count: limitCount }
  const widgetsRef = useRef<Widget[]>([])
  widgetsRef.current = state?.settings.sidebar_widgets ?? []

  // Mirror chrome theme changes onto theme-sensitive widget URLs.
  useEffect(() => {
    let lastMode = document.body.getAttribute('data-mode')
    const observer = new MutationObserver(() => {
      const mode = document.body.getAttribute('data-mode')
      // The theme hook re-sets the attribute on unrelated settings changes;
      // only an actual light/dark flip warrants a navigation.
      if (mode === lastMode) return
      lastMode = mode
      const dark = mode === 'dark'
      for (const id of liveRef.current.keys()) {
        const widget = widgetsRef.current.find((w) => w.id === id)
        if (!widget) continue
        const url = effectiveWidgetUrl(widget, dark)
        if (url === effectiveWidgetUrl(widget, !dark)) continue
        void commands.navigateWidgetPanel(widget.id, url).catch(() => undefined)
      }
    })
    observer.observe(document.body, { attributes: true, attributeFilter: ['data-mode'] })
    return () => observer.disconnect()
  }, [])

  // Pre-create the chrome companion overlays so the first panel open doesn't
  // render before them.
  useEffect(() => {
    void commands.ensureOverlay('widgetresize').catch(() => undefined)
    void commands.ensureOverlay('widgetheader').catch(() => undefined)
  }, [])

  const clearTimer = useCallback((id: WidgetId) => {
    const t = timersRef.current.get(id)
    if (t !== undefined) {
      window.clearTimeout(t)
      timersRef.current.delete(id)
    }
  }, [])

  /** Hide the panel's chrome companions (resize grip + header bar). */
  const hideGrip = useCallback(() => {
    void hideGripOverlay().catch(() => undefined)
    void hideHeaderOverlay().catch(() => undefined)
  }, [hideGripOverlay, hideHeaderOverlay])

  /** Show/position the panel's chrome companions for the given geometry. */
  const showChrome = useCallback(
    async (geo: { grip: Rect; header: Rect }) => {
      await showGrip(geo.grip)
      await showHeader(geo.header)
    },
    [showGrip, showHeader],
  )

  /** Animation handle for the panel's grow-outward entrance. */
  const panelAnimRef = useRef<number | null>(null)
  const cancelPanelAnim = useCallback(() => {
    if (panelAnimRef.current !== null) {
      cancelAnimationFrame(panelAnimRef.current)
      panelAnimRef.current = null
    }
  }, [])

  /**
   * Slide the panel in from the dock edge at full size (position-only, like
   * the split-pane entrance, so the page never reflows mid-animation; plain
   * ease-out, no spring). The header tracks the same offset, written via the
   * raw bounds command first (already ensured by showChrome) so it lands in
   * the same IPC drain as the panel write. id null animates the header
   * alone: create paths reveal the panel itself at full size on load.
   */
  const animatePanelIn = useCallback(
    (id: WidgetId | null, panel: Rect) => {
      cancelPanelAnim()
      const dir = sideRef.current === 'left' ? -1 : 1
      const dist = panelSlideDistance(panel.width)
      const t0 = performance.now()
      const step = (now: number) => {
        const t = Math.min(1, (now - t0) / PANEL_ANIM_MS)
        const e = 1 - Math.pow(1 - t, 3)
        const x = Math.round(panel.x + dir * dist * (1 - e))
        void commands
          .setOverlayBounds(
            'widgetheader', x, panel.y - HEADER_HEIGHT, panel.width, HEADER_HEIGHT,
          )
          .catch(() => undefined)
        if (id !== null) {
          void commands
            .setWidgetPanelBounds(id, x, panel.y, panel.width, panel.height)
            .catch(() => undefined)
        }
        panelAnimRef.current = t < 1 ? requestAnimationFrame(step) : null
      }
      panelAnimRef.current = requestAnimationFrame(step)
    },
    [cancelPanelAnim],
  )

  /** Destroy one panel webview and forget it. */
  const destroyOne = useCallback(
    (id: WidgetId) => {
      clearTimer(id)
      liveRef.current.delete(id)
      void commands.closeWidgetPanel(id).catch(() => undefined)
      if (visibleRef.current === id) {
        cancelPanelAnim()
        visibleRef.current = null
        setActiveWidgetId(null)
        setPanelRect(null)
        hideGrip()
      }
    },
    [clearTimer, hideGrip],
  )

  /** Destroy every live panel (workspace switch, unmount). */
  const destroyAll = useCallback(() => {
    cancelPanelAnim()
    for (const t of timersRef.current.values()) window.clearTimeout(t)
    timersRef.current.clear()
    liveRef.current.clear()
    void commands.closeAllWidgetPanels().catch(() => undefined)
    if (visibleRef.current !== null) {
      visibleRef.current = null
      setActiveWidgetId(null)
    }
    setPanelRect(null)
    hideGrip()
  }, [hideGrip])

  /** Arm the idle-destroy timer for a panel that just became hidden. */
  const armIdleTimer = useCallback(
    (id: WidgetId) => {
      clearTimer(id)
      if (!sleepRef.current.keepLoaded) return
      timersRef.current.set(
        id,
        window.setTimeout(() => destroyOne(id), sleepRef.current.minutes * 60_000),
      )
    },
    [clearTimer, destroyOne],
  )

  /** Hide a panel, keeping it live for an instant re-show; with
   * keep-loaded off, destroy outright so its process is reclaimed now. */
  const hideOne = useCallback(
    (id: WidgetId) => {
      if (!sleepRef.current.keepLoaded) {
        destroyOne(id)
        return
      }
      void commands.hideWidgetPanel(id).catch(() => undefined)
      armIdleTimer(id)
    },
    [armIdleTimer, destroyOne],
  )

  /**
   * Dismiss the visible panel: hide it (state kept) and arm its idle-destroy
   * timer. Private workspaces destroy outright instead.
   */
  const dismiss = useCallback(() => {
    const visible = visibleRef.current
    if (visible === null) return
    if (ephemeral) {
      destroyOne(visible)
      return
    }
    cancelPanelAnim()
    visibleRef.current = null
    setActiveWidgetId(null)
    setPanelRect(null)
    hideGrip()
    hideOne(visible)
  }, [cancelPanelAnim, ephemeral, destroyOne, hideGrip, hideOne])

  /**
   * Evict least-recently-used hidden panels until at most `target` remain.
   * The visible panel is never evicted; stops early when only it is left.
   */
  const evictDownTo = useCallback(
    (target: number) => {
      while (liveRef.current.size > target) {
        let oldest: WidgetId | null = null
        let oldestAt = Number.POSITIVE_INFINITY
        for (const [id, at] of liveRef.current) {
          if (id === visibleRef.current) continue
          if (at < oldestAt) {
            oldestAt = at
            oldest = id
          }
        }
        if (oldest === null) return
        destroyOne(oldest)
      }
    },
    [destroyOne],
  )

  const toggleWidget = useCallback(
    (widget: Widget) => {
      if (visibleRef.current === widget.id) {
        dismiss()
        return
      }
      const geo = computeGeometry(sideRef.current, widthRef.current)
      if (!geo) return
      clearTimer(widget.id)

      const previous = visibleRef.current
      const fail = () => {
        liveRef.current.delete(widget.id)
        if (visibleRef.current === widget.id) {
          visibleRef.current = null
          setActiveWidgetId(null)
          setPanelRect(null)
          hideGrip()
        }
      }
      visibleRef.current = widget.id
      setActiveWidgetId(widget.id)
      setPanelRect(geo.panel)

      const wasLive = liveRef.current.has(widget.id)
      if (!wasLive && limitRef.current.enabled) {
        // Make room for the incoming panel.
        evictDownTo(Math.max(1, limitRef.current.count) - 1)
      }
      liveRef.current.set(widget.id, Date.now())

      const { panel } = geo
      const dark = document.body.getAttribute('data-mode') === 'dark'
      const url = effectiveWidgetUrl(widget, dark)
      const run = async () => {
        // Hide the previous panel first: panels created later stack higher,
        // so revealing the target under a still-visible newer panel would
        // leave it covered.
        const stale = () => visibleRef.current !== widget.id
        if (previous !== null) hideOne(previous)
        await showChrome(geo)
        if (stale()) {
          hideGrip()
          return
        }
        const off = document.body.classList.contains('motion-off')
        const grow = !off && wasLive
        // Create paths: slide the header strip in step with the backdrop.
        if (!off && !grow) animatePanelIn(null, panel)
        const dir = sideRef.current === 'left' ? -1 : 1
        const sx = grow ? panel.x + dir * panelSlideDistance(panel.width) : panel.x
        if (wasLive) {
          const shown = await commands.showWidgetPanel(
            widget.id, sx, panel.y, panel.width, panel.height,
          )
          if (stale()) {
            hideOne(widget.id)
            hideGrip()
            return
          }
          if (shown) {
            if (grow) animatePanelIn(widget.id, panel)
          } else {
            await commands.openWidgetPanel(
              widget.id, url, panel.x, panel.y, panel.width, panel.height, dark,
            )
          }
        } else {
          await commands.openWidgetPanel(
            widget.id, url, panel.x, panel.y, panel.width, panel.height, dark,
          )
        }
        if (stale()) {
          hideOne(widget.id)
          hideGrip()
        }
      }
      void run().catch(fail)
    },
    [
      dismiss,
      clearTimer,
      evictDownTo,
      hideOne,
      showChrome,
      hideGrip,
      animatePanelIn,
      cancelPanelAnim,
    ],
  )

  // Grip drags from the overlay surface: resize the visible panel live,
  // anchored at its outer edge; persist + reposition the grip on drag end.
  useEffect(() => {
    const apply = (dx: number, end: boolean) => {
      const base = resizeBaseRef.current
      const visible = visibleRef.current
      if (!base || visible === null) return
      const s = sideRef.current
      const w = clampWidth(s === 'left' ? base.startW + dx : base.startW - dx)
      widthRef.current = w
      const x = s === 'left' ? base.outerEdge : base.outerEdge - w
      void commands
        .setWidgetPanelBounds(visible, Math.round(x), base.y, w, base.height)
        .catch(() => undefined)
      setPanelRect({ x: Math.round(x), y: base.y, width: w, height: base.height })
      // The header bar tracks the panel's top edge live during the drag.
      void showHeader({
        x: Math.round(x),
        y: base.y - HEADER_HEIGHT,
        width: w,
        height: HEADER_HEIGHT,
      }).catch(() => undefined)
      if (end) {
        resizeBaseRef.current = null
        try {
          localStorage.setItem(WIDTH_STORAGE_KEY, String(w))
        } catch {
          /* ignore */
        }
        const geo = computeGeometry(s, w)
        if (geo) void showChrome(geo).catch(() => undefined)
      }
    }

    const unlisteners: UnlistenFn[] = []
    let cancelled = false
    const track = (p: Promise<UnlistenFn>) =>
      void p.then((fn) => (cancelled ? fn() : unlisteners.push(fn)))

    track(
      listen(WRESIZE_START_EVENT, () => {
        cancelPanelAnim()
        if (visibleRef.current === null) return
        const geo = computeGeometry(sideRef.current, widthRef.current)
        if (!geo) return
        const outerEdge =
          sideRef.current === 'left' ? geo.panel.x : geo.panel.x + geo.panel.width
        resizeBaseRef.current = {
          startW: widthRef.current,
          outerEdge,
          y: geo.panel.y,
          height: geo.panel.height,
        }
      }),
    )
    track(listen<number>(WRESIZE_MOVE_EVENT, (e) => apply(e.payload, false)))
    track(listen<number>(WRESIZE_END_EVENT, (e) => apply(e.payload, true)))
    // Reload click from the header surface: native, cache-bypassing.
    track(
      listen(WPANEL_RELOAD_EVENT, () => {
        const visible = visibleRef.current
        if (visible !== null) void commands.reloadWidgetPanel(visible).catch(() => undefined)
      }),
    )
    // A panel navigated: cache its URL and refresh the header if it is visible.
    track(
      listen<WPanelNavPayload>(WPANEL_NAV_EVENT, (e) => {
        const id = e.payload.id as WidgetId
        panelUrlsRef.current.set(id, e.payload.url)
        if (id === visibleRef.current) pushHeaderState(id)
      }),
    )
    // Mute toggle from the header surface: engine-level page mute, so
    // messaging pings stay silent too. The new state echoes back to the
    // surface so its icon flips.
    track(
      listen(WPANEL_MUTE_EVENT, () => {
        const visible = visibleRef.current
        if (visible === null) return
        void commands
          .widgetPanelMuted(visible)
          .then(async (muted) => {
            await commands.setWidgetPanelMuted(visible, !muted)
            await emit(WPANEL_STATE_EVENT, {
              muted: !muted,
              url: panelUrlsRef.current.get(visible) ?? '',
            })
          })
          .catch(() => undefined)
      }),
    )
    return () => {
      cancelled = true
      for (const un of unlisteners) un()
    }
  }, [showGrip, showHeader, showChrome, pushHeaderState])

  // Dismiss on chrome interaction outside the widget dock.
  useEffect(() => {
    const onPointerDown = (e: PointerEvent) => {
      if (visibleRef.current === null) return
      const target = e.target as Element | null
      if (target?.closest?.('.kb-widget-dock')) return
      dismiss()
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') dismiss()
    }
    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
    }
  }, [dismiss])

  // Dismiss when a click lands inside a tab webview (invisible to the chrome DOM).
  useEffect(() => {
    let un: UnlistenFn | null = null
    let cancelled = false
    void listen<string>(PANE_FOCUS_EVENT, () => dismiss()).then((fn) =>
      cancelled ? fn() : (un = fn),
    )
    return () => {
      cancelled = true
      un?.()
    }
  }, [dismiss])

  // Flipping keep-loaded off reclaims every hidden panel on the spot (the
  // visible one stays and is destroyed when dismissed).
  useEffect(() => {
    if (keepLoaded) return
    for (const id of Array.from(liveRef.current.keys())) {
      if (id !== visibleRef.current) destroyOne(id)
    }
  }, [keepLoaded, destroyOne])

  // Memory-pressure eviction: when the hidden panels' combined WebContent
  // footprint exceeds the cap, reclaim them oldest-first until back under.
  // Complements the count cap with a size-based one.
  useEffect(() => {
    if (!keepLoaded || !memCapEnabled) return
    let cancelled = false
    const sweep = async () => {
      const hidden = Array.from(liveRef.current.entries()).filter(
        ([id]) => id !== visibleRef.current,
      )
      if (hidden.length === 0) return
      const rows = await commands.listWebviewMemory().catch(() => null)
      if (!rows || cancelled) return
      const prefix = 'widget-panel-'
      const bytesById = new Map(
        rows
          .filter((r) => r.label.startsWith(prefix))
          .map((r) => [r.label.slice(prefix.length), r.bytes] as const),
      )
      let total = 0
      for (const [id] of hidden) total += bytesById.get(String(id)) ?? 0
      const capBytes = memCapMbRef.current * 1024 * 1024
      if (total <= capBytes) return
      const lru = [...hidden].sort((a, b) => a[1] - b[1])
      for (const [id] of lru) {
        if (total <= capBytes) break
        total -= bytesById.get(String(id)) ?? 0
        destroyOne(id)
      }
    }
    const t = window.setInterval(() => void sweep(), 30_000)
    void sweep()
    return () => {
      cancelled = true
      window.clearInterval(t)
    }
  }, [keepLoaded, memCapEnabled, destroyOne])

  // Reflect limit changes immediately: trim the pool as soon as the cap is
  // lowered (or re-enabled).
  useEffect(() => {
    if (limitEnabled) evictDownTo(Math.max(1, limitCount))
  }, [limitEnabled, limitCount, evictDownTo])

  // Panels are bound to one workspace's data store + proxy: hard-destroy all
  // on workspace switch so state never leaks across the privacy boundary.
  useEffect(() => {
    destroyAll()
  }, [wsId, destroyAll])

  // An active tab change is "moved on" but stays in-workspace: just dismiss.
  const activeTabId = state?.workspaces.find((w) => w.id === wsId)?.active_tab_id ?? null
  useEffect(() => {
    dismiss()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTabId])

  // Follow the content host while visible: the collapsible sidebar's
  // expand/collapse transition moves the host edge frame by frame, and the
  // panel (with its header, grip and backdrop) must retract along with it
  // instead of staying where it opened.
  useEffect(() => {
    if (activeWidgetId === null) return
    const host = document.querySelector('.kb-content-host')
    if (!host) return
    let last: DOMRect | null = null
    const apply = () => {
      const r = host.getBoundingClientRect()
      const unchanged =
        last !== null &&
        Math.abs(r.left - last.left) < 0.5 &&
        Math.abs(r.top - last.top) < 0.5 &&
        Math.abs(r.width - last.width) < 0.5 &&
        Math.abs(r.height - last.height) < 0.5
      const first = last === null
      last = r
      if (first || unchanged) return
      if (resizeBaseRef.current) return // a grip drag owns the bounds
      const geo = computeGeometry(sideRef.current, widthRef.current)
      if (!geo) return
      cancelPanelAnim()
      void commands
        .setWidgetPanelBounds(
          activeWidgetId, geo.panel.x, geo.panel.y, geo.panel.width, geo.panel.height,
        )
        .catch(() => undefined)
      void commands
        .setOverlayBounds(
          'widgetheader', geo.header.x, geo.header.y, geo.header.width, geo.header.height,
        )
        .catch(() => undefined)
      void commands
        .setOverlayBounds('widgetresize', geo.grip.x, geo.grip.y, geo.grip.width, geo.grip.height)
        .catch(() => undefined)
      setPanelRect(geo.panel)
    }
    const ro = new ResizeObserver(apply)
    ro.observe(host)
    return () => ro.disconnect()
  }, [activeWidgetId, cancelPanelAnim])

  // Track window resizes while visible.
  useEffect(() => {
    if (activeWidgetId === null) return
    const onResize = () => {
      widthRef.current = clampWidth(widthRef.current)
      const geo = computeGeometry(sideRef.current, widthRef.current)
      if (!geo) return
      void commands
        .setWidgetPanelBounds(
          activeWidgetId, geo.panel.x, geo.panel.y, geo.panel.width, geo.panel.height,
        )
        .catch(() => undefined)
      setPanelRect(geo.panel)
      void showChrome(geo).catch(() => undefined)
    }
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [activeWidgetId, showChrome])

  // Reclaim every webview when the dock unmounts (e.g. layout switch).
  useEffect(() => destroyAll, [destroyAll])

  return { activeWidgetId, toggleWidget, panelRect, panelSide: side }
}
