/**
 * Chrome-side orchestration for the window-region selection overlay. The
 * `capture_region` action dispatches `SCREENSHOT_REGION_OPEN_EVENT` (a DOM
 * event carrying the active tab id); this hook shows a full-window
 * `overlay-screenshot-region` webview hosting `ScreenshotRegionSurface` and
 * tells it to begin. When the surface reports a rect, the hook captures the
 * window cropped to it (clipboard) and triggers the success toast; a `null`
 * rect (cancel) just collapses the overlay.
 */

import { useCallback, useEffect, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { TabId } from '@/bindings/types'
import { currentPlatform } from '@/lib/platform'
import { useOverlay } from '@/hooks/useOverlay'
import {
  SCREENSHOT_CAPTURED_EVENT,
  SCREENSHOT_REGION_BEGIN_EVENT,
  SCREENSHOT_REGION_OPEN_EVENT,
  SCREENSHOT_REGION_READY_EVENT,
  SCREENSHOT_REGION_RESULT_EVENT,
  type RegionRect,
} from '@/lib/screenshot'

export function useScreenshotRegionOverlay(): void {
  const overlay = useOverlay('screenshot-region')
  // True while a selection is in flight, so a late surface-ready can replay the
  // "begin" the lazily-created webview missed.
  const activeRef = useRef(false)
  const tabIdRef = useRef<TabId | null>(null)

  const fullRect = () => ({
    x: 0,
    y: 0,
    width: window.innerWidth,
    height: window.innerHeight,
  })

  const open = useCallback(
    async (tabId: TabId) => {
      tabIdRef.current = tabId
      activeRef.current = true
      await overlay.show(fullRect())
      void emit(SCREENSHOT_REGION_BEGIN_EVENT).catch(() => undefined)
    },
    [overlay],
  )

  useEffect(() => {
    const handler = (e: Event) => {
      const tabId = (e as CustomEvent<{ tabId?: TabId }>).detail?.tabId
      if (tabId) void open(tabId)
    }
    window.addEventListener(SCREENSHOT_REGION_OPEN_EVENT, handler)
    return () => window.removeEventListener(SCREENSHOT_REGION_OPEN_EVENT, handler)
  }, [open])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<void>(SCREENSHOT_REGION_READY_EVENT, () => {
      if (activeRef.current) void emit(SCREENSHOT_REGION_BEGIN_EVENT).catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<{ rect: RegionRect | null }>(SCREENSHOT_REGION_RESULT_EVENT, (e) => {
      void (async () => {
        activeRef.current = false
        await overlay.hide()
        const rect = e.payload.rect
        const tabId = tabIdRef.current
        if (!rect || !tabId) return
        let cropRect: RegionRect | null = rect
        if (currentPlatform() === 'windows') {
          cropRect = toViewportFraction(rect, tabId)
          if (!cropRect) return
        }
        try {
          const captured = await commands.captureToClipboard(tabId, 'region', cropRect)
          if (captured) window.dispatchEvent(new CustomEvent(SCREENSHOT_CAPTURED_EVENT))
        } catch (err) {
          // eslint-disable-next-line no-console
          console.error('captureToClipboard (region) failed', err)
        }
      })()
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [overlay])

  useEffect(() => {
    const onResize = () => {
      if (activeRef.current) void overlay.show(fullRect())
    }
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [overlay])
}

/**
 * Map a window-space selection to [0,1] fractions of the tab's viewport (the
 * `[data-pane-leaf]` element the webview is sized to). Returns null when the
 * selection does not overlap the viewport. Fractions keep the Rust-side crop
 * DPI-independent.
 */
function toViewportFraction(rect: RegionRect, tabId: TabId): RegionRect | null {
  const el = document.querySelector<HTMLElement>(`[data-pane-leaf="${tabId}"]`)
  const host = el?.getBoundingClientRect()
  if (!host || host.width <= 0 || host.height <= 0) return null

  const left = Math.max(rect.x, host.left)
  const top = Math.max(rect.y, host.top)
  const right = Math.min(rect.x + rect.width, host.right)
  const bottom = Math.min(rect.y + rect.height, host.bottom)
  const iw = right - left
  const ih = bottom - top
  if (iw <= 0 || ih <= 0) return null

  return {
    x: (left - host.left) / host.width,
    y: (top - host.top) / host.height,
    width: iw / host.width,
    height: ih / host.height,
  }
}
