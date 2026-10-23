/**
 * General message toast, hosted by the `overlay-toast` webview. Drops down at
 * the window's top-right, shows a short message (e.g. a feature that is not
 * available on the running platform), and auto-dismisses, paused while hovered.
 *
 * The message is pushed from the chrome over `TOAST_SHOW_EVENT`; the surface
 * announces `TOAST_READY_EVENT` on mount so the chrome can replay a "show" that
 * raced this lazily-created webview.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { cx } from '@/lib/cx'
import { XIcon } from '@/lib/icons'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { menusGlass } from '@/lib/glass'
import {
  TOAST_CLOSED_EVENT,
  TOAST_READY_EVENT,
  TOAST_SHOW_EVENT,
  type ToastRequest,
} from '@/lib/toast'

/** Visible dwell before auto-dismiss, and the dwell after a hover ends. */
const STAY_MS = 3200
const RESUME_MS = 1500
/** Matches the reverse-out CSS animation duration. */
const REVERSE_MS = 220

export function ToastSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const glassMenus = menusGlass(state?.settings)

  const [message, setMessage] = useState('')
  const [open, setOpen] = useState(false)
  const [closing, setClosing] = useState(false)
  // The dwell-before-dismiss timer and the reverse-animation-completion timer.
  const dwellRef = useRef<number | null>(null)
  const closeRef = useRef<number | null>(null)

  const clearDwell = () => {
    if (dwellRef.current !== null) {
      window.clearTimeout(dwellRef.current)
      dwellRef.current = null
    }
  }
  const clearClose = () => {
    if (closeRef.current !== null) {
      window.clearTimeout(closeRef.current)
      closeRef.current = null
    }
  }

  const dismiss = useCallback(() => {
    clearDwell()
    setClosing(true)
    clearClose()
    closeRef.current = window.setTimeout(() => {
      setOpen(false)
      setClosing(false)
      void emit(TOAST_CLOSED_EVENT).catch(() => undefined)
      void commands.hideOverlay('toast').catch(() => undefined)
    }, REVERSE_MS)
  }, [])

  const armDismiss = useCallback(
    (ms: number) => {
      clearDwell()
      dwellRef.current = window.setTimeout(() => dismiss(), ms)
    },
    [dismiss],
  )

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    void listen<ToastRequest>(TOAST_SHOW_EVENT, (e) => {
      clearClose()
      setClosing(false)
      setMessage(e.payload.message)
      setOpen(true)
      armDismiss(STAY_MS)
    }).then((fn) => {
      unlisten = fn
      void emit(TOAST_READY_EVENT).catch(() => undefined)
    })
    return () => {
      unlisten?.()
      clearDwell()
      clearClose()
    }
  }, [armDismiss])

  if (!open) return null

  return (
    <div
      className={cx('kb-toast-wrap', { 'glass-menus': glassMenus })}
      onMouseEnter={clearDwell}
      onMouseLeave={() => armDismiss(RESUME_MS)}
    >
      <div className={cx('kb-toast', { 'is-closing': closing })}>
        <span className="kb-toast-msg">{message}</span>
        <button
          type="button"
          className="kb-toast-x"
          onClick={dismiss}
          aria-label="Dismiss"
        >
          <XIcon />
        </button>
      </div>
    </div>
  )
}
