/**
 * Right-click options popover for a top-strip tab island. The menu lives in the
 * `overlay-islandmenu` webview so it composites above the page (a chrome-drawn
 * menu would sit behind the tab webview). Mirrors the `overlay-tabmenu` flow.
 */

import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { AccentColor, TabFolderId, WorkspaceId } from '@/bindings/types'

export interface IslandMenuTarget {
  workspaceId: WorkspaceId
  folderId: TabFolderId
  accent: AccentColor
  name: string
}

export const ISLAND_MENU_OPEN = 'wis-islandmenu-open'
export const ISLAND_MENU_READY = 'wis-islandmenu-ready'
export const ISLAND_MENU_DONE = 'wis-islandmenu-done'

const MENU_W = 240
const MENU_H = 126
const MARGIN = 8
const PANE_FOCUS_EVENT = 'wis-pane-focus'

export function useIslandMenu(): {
  openIslandMenu: (e: ReactMouseEvent, target: IslandMenuTarget) => void
} {
  const ensuredRef = useRef(false)
  const pendingRef = useRef<IslandMenuTarget | null>(null)
  const [open, setOpen] = useState(false)

  // The surface announces readiness on first mount; re-emit the pending open.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen(ISLAND_MENU_READY, () => {
      if (pendingRef.current) void emit(ISLAND_MENU_OPEN, pendingRef.current)
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
    void listen(ISLAND_MENU_DONE, () => {
      pendingRef.current = null
      setOpen(false)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  // Dismiss on a click elsewhere, Escape, or a page click.
  useEffect(() => {
    if (!open) return
    const dismiss = () => {
      pendingRef.current = null
      setOpen(false)
      void commands.hideOverlay('islandmenu').catch(() => undefined)
    }
    const onPointerDown = (e: PointerEvent) => {
      if (e.button === 2 && (e.target as HTMLElement | null)?.closest?.('[data-island-id]')) {
        return
      }
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
  }, [open])

  const openIslandMenu = (e: ReactMouseEvent, target: IslandMenuTarget) => {
    e.preventDefault()
    const x = Math.min(Math.max(MARGIN, e.clientX), window.innerWidth - MENU_W - MARGIN)
    const y = Math.min(Math.max(MARGIN, e.clientY), window.innerHeight - MENU_H - MARGIN)
    pendingRef.current = target

    const place = async () => {
      if (!ensuredRef.current) {
        await commands.ensureOverlay('islandmenu')
        ensuredRef.current = true
      }
      await commands.setOverlayBounds('islandmenu', x, y, MENU_W, MENU_H)
    }
    void place().catch(() => undefined)
    void emit(ISLAND_MENU_OPEN, target)
    setOpen(true)
  }

  return { openIslandMenu }
}
