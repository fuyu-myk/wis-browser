/**
 * Chrome-side orchestration for the screenshot success toast. A successful
 * capture (already copied to the clipboard) dispatches `SCREENSHOT_CAPTURED_EVENT`
 * (a DOM event); this hook shows a small `overlay-screenshot-toast` webview
 * pinned to the window's top-right hosting `ScreenshotToastSurface`, and emits
 * `SCREENSHOT_TOAST_SHOWN_EVENT`. The "shown" is replayed when the surface
 * announces `SCREENSHOT_TOAST_READY_EVENT`, covering the first-capture race
 * where the overlay webview was only just created. The surface auto-dismisses
 * and broadcasts `SCREENSHOT_TOAST_CLOSED_EVENT` so this hook collapses it.
 */

import { useEffect, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { useOverlay } from '@/hooks/useOverlay'
import {
  SCREENSHOT_CAPTURED_EVENT,
  SCREENSHOT_TOAST_CLOSED_EVENT,
  SCREENSHOT_TOAST_READY_EVENT,
  SCREENSHOT_TOAST_SHOWN_EVENT,
} from '@/features/screenshot/screenshot'

/** Top-right region the toast webview occupies (logical px). Wider/taller than
 *  the card so the drop-in animation isn't clipped on its sides. */
const TOAST_W = 400
const TOAST_H = 140

export function useScreenshotToastOverlay(): void {
  const overlay = useOverlay('screenshot-toast')
  // True between a capture and its toast dismissing, so a late surface-ready
  // can replay the "shown" the first capture missed.
  const armedRef = useRef(false)

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []

    const rect = () => ({
      x: Math.max(0, window.innerWidth - TOAST_W),
      y: 0,
      width: TOAST_W,
      height: TOAST_H,
    })

    const onCaptured = () => {
      void (async () => {
        armedRef.current = true
        await overlay.show(rect())
        void emit(SCREENSHOT_TOAST_SHOWN_EVENT).catch(() => undefined)
      })()
    }
    window.addEventListener(SCREENSHOT_CAPTURED_EVENT, onCaptured)

    void listen<void>(SCREENSHOT_TOAST_READY_EVENT, () => {
      if (armedRef.current) void emit(SCREENSHOT_TOAST_SHOWN_EVENT).catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    void listen<void>(SCREENSHOT_TOAST_CLOSED_EVENT, () => {
      armedRef.current = false
      void overlay.hide().catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    return () => {
      cancelled = true
      window.removeEventListener(SCREENSHOT_CAPTURED_EVENT, onCaptured)
      unlistens.forEach((fn) => fn())
    }
  }, [overlay])
}
