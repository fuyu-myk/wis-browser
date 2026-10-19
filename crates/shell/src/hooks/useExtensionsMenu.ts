/**
 * The extensions menu under the address bar's puzzle button. It lives in the
 * `overlay-extmenu` webview so it composites above the page, like the island
 * menu, and carries the button's rectangle so a popup can anchor to it. The
 * same overlay asks when a running extension wants more access, one request
 * at a time.
 */

import { useCallback, useEffect, useRef, useState, type RefObject } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { ExtensionAction, ExtensionPermissionRequest, WorkspaceId } from '@/bindings/types'
import {
  EXT_MENU_DONE,
  EXT_MENU_OPEN,
  EXT_MENU_READY,
  EXT_PROMPT_SIZE,
  PROMPT_HEIGHT,
  menuBounds,
  promptBounds,
  type Box,
} from '@/lib/extensionsMenu'

export type ExtensionsMenuPayload =
  | { kind: 'menu'; anchor: Box; actions: ExtensionAction[]; workspaceId: WorkspaceId | null }
  | { kind: 'prompt'; request: ExtensionPermissionRequest }

const PANE_FOCUS_EVENT = 'wis-pane-focus'
const PERMISSION_REQUEST_EVENT = 'wis-extension-permission-request'
const NEXT_PROMPT_DELAY_MS = 60

/** Marks the button, whose own press toggles the menu instead of dismissing it. */
export const EXT_BUTTON_ATTR = 'data-extensions-button'

function keep(cancelled: () => boolean, unlistens: UnlistenFn[]) {
  return (fn: UnlistenFn) => (cancelled() ? fn() : unlistens.push(fn))
}

function viewport() {
  return { width: window.innerWidth, height: window.innerHeight }
}

export function useExtensionsMenu(
  workspaceId: WorkspaceId | null,
  buttonRef: RefObject<HTMLElement>,
): {
  toggle: (button: HTMLElement) => void
} {
  const ensuredRef = useRef(false)
  const pendingRef = useRef<ExtensionsMenuPayload | null>(null)
  const promptsRef = useRef<ExtensionPermissionRequest[]>([])
  const [open, setOpen] = useState(false)

  const ensure = useCallback(async () => {
    if (ensuredRef.current) return
    await commands.ensureOverlay('extmenu')
    ensuredRef.current = true
  }, [])

  const placePrompt = useCallback((height: number) => {
    const rect = buttonRef.current?.getBoundingClientRect()
    const anchor = rect
      ? { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
      : { x: window.innerWidth - 8, y: 0, width: 0, height: 0 }
    const bounds = promptBounds(anchor, height, viewport())
    return commands.setOverlayBounds('extmenu', bounds.x, bounds.y, bounds.width, bounds.height)
  }, [buttonRef])

  const showPrompt = useCallback(async () => {
    const request = promptsRef.current[0]
    if (!request) return
    setOpen(false)
    await ensure()
    await placePrompt(PROMPT_HEIGHT)
    const payload: ExtensionsMenuPayload = { kind: 'prompt', request }
    pendingRef.current = payload
    void emit(EXT_MENU_OPEN, payload)
  }, [ensure, placePrompt])

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []
    const kept = keep(() => cancelled, unlistens)

    void listen(EXT_MENU_READY, () => {
      if (pendingRef.current) void emit(EXT_MENU_OPEN, pendingRef.current)
    }).then(kept)

    void listen(EXT_MENU_DONE, () => {
      const wasPrompt = pendingRef.current?.kind === 'prompt'
      pendingRef.current = null
      setOpen(false)
      if (!wasPrompt) return
      promptsRef.current.shift()
      if (promptsRef.current.length > 0) {
        window.setTimeout(() => void showPrompt().catch(() => undefined), NEXT_PROMPT_DELAY_MS)
      }
    }).then(kept)

    void listen<{ request: number; height: number }>(EXT_PROMPT_SIZE, (e) => {
      const current = pendingRef.current
      if (current?.kind === 'prompt' && current.request.request === e.payload.request) {
        void placePrompt(e.payload.height).catch(() => undefined)
      }
    }).then(kept)

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
    }
  }, [showPrompt, placePrompt])

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []
    void listen<ExtensionPermissionRequest>(PERMISSION_REQUEST_EVENT, (e) => {
      if (e.payload.workspace_id !== workspaceId) return
      promptsRef.current.push(e.payload)
      if (promptsRef.current.length === 1) void showPrompt().catch(() => undefined)
    }).then(keep(() => cancelled, unlistens))
    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
    }
  }, [workspaceId, showPrompt])

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
    if (promptsRef.current.length > 0) return
    if (open) {
      dismiss()
      return
    }
    const rect = button.getBoundingClientRect()
    const anchor = { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
    const show = async () => {
      if ((await commands.extensionDismissPopup()) !== null) return
      const actions = await commands.extensionActions()
      const payload: ExtensionsMenuPayload = { kind: 'menu', anchor, actions, workspaceId }
      const siteLines = actions.filter((a) => a.site).length
      const bounds = menuBounds(anchor, actions.length, viewport(), siteLines)
      await ensure()
      await commands.setOverlayBounds('extmenu', bounds.x, bounds.y, bounds.width, bounds.height)
      pendingRef.current = payload
      void emit(EXT_MENU_OPEN, payload)
      setOpen(true)
    }
    void show().catch(() => undefined)
  }

  return { toggle }
}
