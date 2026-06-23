/**
 * Screenshot success toast, hosted by the `overlay-screenshot-toast` webview.
 * Drops down at the window's top-right after a capture, says it was copied to
 * the clipboard, and offers a Save button (the downloads icon) to write the
 * image to disk. Auto-dismisses after a few seconds, paused while hovered, then
 * reverses out.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { cx } from '@/lib/cx'
import { DownloadIcon, XIcon } from '@/lib/icons'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { menusGlass } from '@/lib/glass'
import {
  SCREENSHOT_TOAST_CLOSED_EVENT,
  SCREENSHOT_TOAST_READY_EVENT,
  SCREENSHOT_TOAST_SHOWN_EVENT,
} from '@/lib/screenshot'

/** Visible dwell before auto-dismiss, and the dwell after a hover ends. */
const STAY_MS = 3000
const RESUME_MS = 1500
/** Matches the reverse-out CSS animation duration. */
const REVERSE_MS = 220

export function ScreenshotToastSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const glassMenus = menusGlass(state?.settings)

  const [open, setOpen] = useState(false)
  const [closing, setClosing] = useState(false)
  const [saved, setSaved] = useState(false)
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
      setSaved(false)
      void emit(SCREENSHOT_TOAST_CLOSED_EVENT).catch(() => undefined)
      void commands.hideOverlay('screenshot-toast').catch(() => undefined)
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
    void listen<void>(SCREENSHOT_TOAST_SHOWN_EVENT, () => {
      // Cancel any in-flight reverse-out so a rapid re-capture re-shows cleanly.
      clearClose()
      setClosing(false)
      setSaved(false)
      setOpen(true)
      armDismiss(STAY_MS)
    }).then((fn) => {
      unlisten = fn
      // Tell the chrome we are listening, so it can replay a "shown" that
      // raced this lazily-created webview's first mount.
      void emit(SCREENSHOT_TOAST_READY_EVENT).catch(() => undefined)
    })
    return () => {
      unlisten?.()
      clearDwell()
      clearClose()
    }
  }, [armDismiss])

  const save = useCallback(async () => {
    clearDwell()
    try {
      const path = await commands.saveLastScreenshot()
      if (path) {
        setSaved(true)
        armDismiss(1200)
      } else {
        armDismiss(RESUME_MS)
      }
    } catch {
      armDismiss(RESUME_MS)
    }
  }, [armDismiss])

  if (!open) return null

  return (
    <div
      className={cx('kb-shot-toast-wrap', { 'glass-menus': glassMenus })}
      onMouseEnter={clearDwell}
      onMouseLeave={() => armDismiss(RESUME_MS)}
    >
      <div className={cx('kb-shot-toast', { 'is-closing': closing })}>
        <span className="kb-shot-toast-msg">
          {saved ? 'Saved to disk' : 'Copied to clipboard'}
        </span>
        {!saved && (
          <button
            type="button"
            className="kb-shot-toast-save"
            onClick={() => void save()}
            aria-label="Save screenshot to disk"
          >
            <DownloadIcon />
            <span>Save</span>
          </button>
        )}
        <button
          type="button"
          className="kb-shot-toast-x"
          onClick={dismiss}
          aria-label="Dismiss"
        >
          <XIcon />
        </button>
      </div>
    </div>
  )
}
