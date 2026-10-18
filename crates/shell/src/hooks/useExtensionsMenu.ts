/**
 * The extensions menu under the address bar's puzzle button. It lives in the
 * `overlay-extmenu` webview so it composites above the page, like the island
 * menu, and carries the button's rectangle so a popup can anchor to it.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { ExtensionAction, WorkspaceId } from '@/bindings/types'
import { EXT_MENU_DONE, EXT_MENU_OPEN, EXT_MENU_READY, menuBounds, type Box } from '@/lib/extensionsMenu'

export interface ExtensionsMenuPayload {
  anchor: Box
  actions: ExtensionAction[]
  workspaceId: WorkspaceId | null
}

const PANE_FOCUS_EVENT = 'wis-pane-focus'

/** Marks the button, whose own press toggles the menu instead of dismissing it. */
export const EXT_BUTTON_ATTR = 'data-extensions-button'

export function useExtensionsMenu(workspaceId: WorkspaceId | null): {
  toggle: (button: HTMLElement) => void
} {
  const ensuredRef = useRef(false)
  const pendingRef = useRef<ExtensionsMenuPayload | null>(null)
  const [open, setOpen] = useState(false)

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen(EXT_MENU_READY, () => {
      if (pendingRef.current) void emit(EXT_MENU_OPEN, pendingRef.current)
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
    void listen(EXT_MENU_DONE, () => {
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

  const dismiss = useCallback(() => {
    pendingRef.current = null
    setOpen(false)
    void commands.hideOverlay('extmenu').catch(() => undefined)
  }, [])

  useEffect(() => {
    if (!open) return
    const onPointerDown = (e: PointerEvent) => {
      if ((e.target as HTMLElement | null)?.closest?.(`[${EXT_BUTTON_ATTR}]`)) return
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

  const toggle = (button: HTMLElement) => {
    if (open) {
      dismiss()
      return
    }
    const rect = button.getBoundingClientRect()
    const anchor = { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
    const show = async () => {
      if (await commands.extensionDismissPopup()) return
      const actions = await commands.extensionActions()
      const payload = { anchor, actions, workspaceId }
      const bounds = menuBounds(anchor, actions.length, {
        width: window.innerWidth,
        height: window.innerHeight,
      })
      if (!ensuredRef.current) {
        await commands.ensureOverlay('extmenu')
        ensuredRef.current = true
      }
      await commands.setOverlayBounds('extmenu', bounds.x, bounds.y, bounds.width, bounds.height)
      pendingRef.current = payload
      void emit(EXT_MENU_OPEN, payload)
      setOpen(true)
    }
    void show().catch(() => undefined)
  }

  return { toggle }
}
