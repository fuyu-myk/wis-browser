/**
 * Hover a tab row for a beat and a preview card (the `tabpreview` overlay
 * webview) appears beside it. Event delegation on `[data-tab-id]` /
 * `[data-mtab-id]` / `[data-pin-id]` covers both tab strips, folder members,
 * and pinned tabs without touching their drag-heavy row renderers. Mounted
 * once in the chrome (`Window.tsx`).
 *
 * The card is a separate native webview, so the pointer leaving the tab row
 * hides it (moving onto the card counts as leaving); any press, context
 * menu, drag start, resize, or window blur hides it too.
 */

import { useEffect, useRef } from 'react'
import { emit } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { LayoutMode, Tab, TabId } from '@/bindings/types'
import { useBrowserState } from '@/hooks/useBrowserState'

export const TABPREVIEW_SHOW_EVENT = 'wis-tabpreview-show'
export const TABPREVIEW_HIDE_EVENT = 'wis-tabpreview-hide'

/** Hover dwell before the card appears. */
const HOVER_DELAY_MS = 1100
const CARD_W = 296
const CARD_H = 246
const GAP = 8
const MARGIN = 8

const TAB_HOST_SELECTOR = '[data-tab-id], [data-mtab-id], [data-pin-id]'

function tabIdOf(el: Element): TabId | null {
  const host = el.closest(TAB_HOST_SELECTOR)
  if (!host) return null
  const raw =
    host.getAttribute('data-tab-id') ??
    host.getAttribute('data-mtab-id') ??
    host.getAttribute('data-pin-id')
  return (raw as TabId) ?? null
}

function cardRect(anchor: DOMRect, layout: LayoutMode): { x: number; y: number } {
  if (layout === 'top_tabs') {
    return {
      x: Math.round(Math.min(Math.max(MARGIN, anchor.left), window.innerWidth - CARD_W - MARGIN)),
      y: Math.round(Math.min(anchor.bottom + GAP, window.innerHeight - CARD_H - MARGIN)),
    }
  }
  const y = Math.round(Math.min(Math.max(MARGIN, anchor.top), window.innerHeight - CARD_H - MARGIN))
  const right = anchor.right + GAP
  if (layout !== 'sidebar_right' && right + CARD_W + MARGIN <= window.innerWidth) {
    return { x: Math.round(right), y }
  }
  return { x: Math.round(Math.max(MARGIN, anchor.left - GAP - CARD_W)), y }
}

export function useTabHoverPreview(): void {
  const { data: state } = useBrowserState()

  const tabsRef = useRef<Tab[]>([])
  tabsRef.current = state?.tabs ?? []
  const layoutRef = useRef<LayoutMode>('sidebar_left')
  layoutRef.current = state?.settings.layout ?? 'sidebar_left'
  const activeWorkspace = state?.workspaces.find((w) => w.id === state.active_workspace_id)
  const activeTabId = activeWorkspace?.active_tab_id ?? null
  const activeTab = activeTabId !== null ? state?.tabs.find((t) => t.id === activeTabId) : null
  const activeLoading = activeTab?.loading ?? false
  const activeTabRef = useRef<TabId | null>(null)
  activeTabRef.current = activeTabId

  // Stash the active tab shortly after each load settles (restored-asleep
  // tabs then preview next launch).
  useEffect(() => {
    if (!activeTabId || activeLoading) return
    const t = window.setTimeout(() => {
      void commands.stashTabPreview(activeTabId).catch(() => undefined)
    }, 1200)
    return () => window.clearTimeout(t)
  }, [activeTabId, activeLoading])

  const timerRef = useRef<number | undefined>(undefined)
  const hoveredRef = useRef<TabId | null>(null)
  const openRef = useRef(false)
  const ensuredRef = useRef(false)

  useEffect(() => {
    const cancelTimer = () => {
      if (timerRef.current !== undefined) {
        window.clearTimeout(timerRef.current)
        timerRef.current = undefined
      }
    }
    const hide = () => {
      cancelTimer()
      hoveredRef.current = null
      if (!openRef.current) return
      openRef.current = false
      void emit(TABPREVIEW_HIDE_EVENT)
      void commands.hideOverlay('tabpreview').catch(() => undefined)
    }
    const show = async (id: TabId, anchor: DOMRect) => {
      const tab = tabsRef.current.find((t) => t.id === id)
      if (!tab) return
      const { x, y } = cardRect(anchor, layoutRef.current)
      if (!ensuredRef.current) {
        try {
          await commands.ensureOverlay('tabpreview')
          ensuredRef.current = true
        } catch {
          return
        }
      }
      await commands.setOverlayBounds('tabpreview', x, y, CARD_W, CARD_H).catch(() => undefined)
      if (hoveredRef.current !== id) {
        void commands.hideOverlay('tabpreview').catch(() => undefined)
        return
      }
      openRef.current = true
      void emit(TABPREVIEW_SHOW_EVENT, { tabId: id, title: tab.title, url: tab.url })
    }

    const onOver = (e: PointerEvent) => {
      if (e.buttons !== 0) return
      const target = e.target instanceof Element ? e.target : null
      let id = target ? tabIdOf(target) : null
      if (id !== null && id === activeTabRef.current) id = null
      if (id === hoveredRef.current) return
      hide()
      if (!id) return
      hoveredRef.current = id
      const host = target?.closest(TAB_HOST_SELECTOR)
      if (!host) return
      timerRef.current = window.setTimeout(() => {
        timerRef.current = undefined
        void show(id, host.getBoundingClientRect())
      }, HOVER_DELAY_MS)
    }
    const onOut = (e: PointerEvent) => {
      if (hoveredRef.current === null) return
      const to = e.relatedTarget instanceof Element ? e.relatedTarget : null
      if (to && tabIdOf(to) === hoveredRef.current) return
      hide()
    }

    document.addEventListener('pointerover', onOver, { capture: true, passive: true })
    document.addEventListener('pointerout', onOut, { capture: true, passive: true })
    document.addEventListener('pointerdown', hide, { capture: true, passive: true })
    document.addEventListener('contextmenu', hide, { capture: true })
    document.addEventListener('scroll', hide, { capture: true, passive: true })
    window.addEventListener('blur', hide)
    window.addEventListener('resize', hide)
    return () => {
      document.removeEventListener('pointerover', onOver, { capture: true })
      document.removeEventListener('pointerout', onOut, { capture: true })
      document.removeEventListener('pointerdown', hide, { capture: true })
      document.removeEventListener('contextmenu', hide, { capture: true })
      document.removeEventListener('scroll', hide, { capture: true })
      window.removeEventListener('blur', hide)
      window.removeEventListener('resize', hide)
      hide()
    }
  }, [])
}
