/**
 * The site panel under the address bar's padlock, in the `overlay-site`
 * webview so it composites above the page. It opens hidden and shows once the
 * surface has measured its height.
 */

import { useCallback, useEffect, useRef, useState, type RefObject } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { Tab } from '@/bindings/types'
import {
  UNMEASURED_PANEL_HEIGHT,
  SITE_PANEL_CLOSE,
  SITE_PANEL_DONE,
  SITE_PANEL_OPEN,
  SITE_PANEL_READY,
  SITE_PANEL_SHOWN,
  SITE_PANEL_SIZE,
  panelBounds,
  type SitePanelPayload,
} from '@/features/site/lib/sitePanel'

const OVERLAY = 'site'
const PANE_FOCUS_EVENT = 'wis-pane-focus'

/** Marks the button, whose own press toggles the panel. */
export const SITE_BUTTON_ATTR = 'data-site-button'

export interface SitePanelSize {
  window: string
  height: number
}

export function useSitePanel(buttonRef: RefObject<HTMLElement>, shown: Tab | null): { toggle: (tab: Tab) => void } {
  const ensuredRef = useRef(false)
  const pendingRef = useRef<SitePanelPayload | null>(null)
  const [open, setOpen] = useState(false)

  const bounds = useCallback(
    (height: number) => {
      const rect = buttonRef.current?.getBoundingClientRect()
      const anchor = rect ? { x: rect.x, y: rect.y, width: rect.width, height: rect.height } : { x: 8, y: 0, width: 0, height: 0 }
      return panelBounds(anchor, height, { width: window.innerWidth, height: window.innerHeight })
    },
    [buttonRef],
  )

  const dismiss = useCallback(() => {
    pendingRef.current = null
    setOpen(false)
    void commands.hideOverlay(OVERLAY).catch(() => undefined)
    void emit(SITE_PANEL_CLOSE, getCurrentWindow().label)
  }, [])

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []
    const keep = (fn: UnlistenFn) => (cancelled ? fn() : unlistens.push(fn))
    const label = getCurrentWindow().label

    void listen(SITE_PANEL_READY, () => {
      if (pendingRef.current) void emit(SITE_PANEL_OPEN, pendingRef.current)
    }).then(keep)

    void listen(SITE_PANEL_DONE, () => {
      pendingRef.current = null
      setOpen(false)
    }).then(keep)

    void listen<SitePanelSize>(SITE_PANEL_SIZE, (e) => {
      if (e.payload.window !== label || !pendingRef.current) return
      const { x, y, width, height } = bounds(e.payload.height)
      void commands
        .setOverlayBounds(OVERLAY, x, y, width, height)
        .then(() => emit(SITE_PANEL_SHOWN, label))
        .catch(() => undefined)
    }).then(keep)

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
    }
  }, [bounds])

  const shownTab = shown?.id
  const shownUrl = shown?.url
  useEffect(() => {
    const panel = pendingRef.current
    if (panel && (panel.tabId !== shownTab || panel.url !== shownUrl)) dismiss()
  }, [shownTab, shownUrl, dismiss])

  useEffect(() => {
    if (!open) return
    const onPointerDown = (e: PointerEvent) => {
      if ((e.target as HTMLElement | null)?.closest?.(`[${SITE_BUTTON_ATTR}]`)) return
      dismiss()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') dismiss()
    }
    window.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKey)
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen(PANE_FOCUS_EVENT, () => dismiss()).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
      window.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open, dismiss])

  const toggle = (tab: Tab) => {
    if (open) {
      dismiss()
      return
    }
    const payload: SitePanelPayload = { window: getCurrentWindow().label, tabId: tab.id, url: tab.url }
    const show = async () => {
      if (!ensuredRef.current) {
        await commands.ensureOverlay(OVERLAY)
        ensuredRef.current = true
      }
      const { x, y, width, height } = bounds(UNMEASURED_PANEL_HEIGHT)
      await commands.sizeOverlay(OVERLAY, x, y, width, height)
      pendingRef.current = payload
      void emit(SITE_PANEL_OPEN, payload)
      setOpen(true)
    }
    void show().catch(() => undefined)
  }

  return { toggle }
}
