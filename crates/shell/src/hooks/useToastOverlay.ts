/**
 * Chrome-side orchestration for the general message toast. A caller anywhere in
 * the chrome dispatches `TOAST_REQUEST_EVENT` (a DOM event carrying the
 * message); this hook shows a small `overlay-toast` webview pinned to the
 * window's top-right hosting `ToastSurface`, and forwards the message over
 * `TOAST_SHOW_EVENT`. The message is replayed when the surface announces
 * `TOAST_READY_EVENT`, covering the first-toast race where the overlay webview
 * was only just created. The surface auto-dismisses and broadcasts
 * `TOAST_CLOSED_EVENT` so this hook collapses it.
 */

import { useEffect, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { useOverlay } from '@/hooks/useOverlay'
import {
  TOAST_CLOSED_EVENT,
  TOAST_READY_EVENT,
  TOAST_REMOTE_EVENT,
  TOAST_REQUEST_EVENT,
  TOAST_SHOW_EVENT,
  showToast,
  type RemoteToastRequest,
  type ToastRequest,
} from '@/lib/toast'

/** Top-right region the toast webview occupies (logical px). Wider than the
 *  card so the drop-in animation isn't clipped on its sides. */
const TOAST_W = 460
const TOAST_H = 96

export function useToastOverlay(): void {
  const overlay = useOverlay('toast')
  const messageRef = useRef<string | null>(null)

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []

    const rect = () => ({
      x: Math.max(0, window.innerWidth - TOAST_W),
      y: 0,
      width: TOAST_W,
      height: TOAST_H,
    })

    const onRequest = (e: Event) => {
      const detail = (e as CustomEvent<ToastRequest>).detail
      if (!detail?.message) return
      messageRef.current = detail.message
      void (async () => {
        await overlay.show(rect())
        void emit(TOAST_SHOW_EVENT, { message: detail.message }).catch(() => undefined)
      })()
    }
    window.addEventListener(TOAST_REQUEST_EVENT, onRequest)

    void listen<RemoteToastRequest>(TOAST_REMOTE_EVENT, (event) => {
      if (!event.payload?.message) return
      if (event.payload.window !== getCurrentWindow().label) return
      showToast(event.payload.message)
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    void listen<void>(TOAST_READY_EVENT, () => {
      if (messageRef.current)
        void emit(TOAST_SHOW_EVENT, { message: messageRef.current }).catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    void listen<void>(TOAST_CLOSED_EVENT, () => {
      messageRef.current = null
      void overlay.hide().catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    return () => {
      cancelled = true
      window.removeEventListener(TOAST_REQUEST_EVENT, onRequest)
      unlistens.forEach((fn) => fn())
    }
  }, [overlay])
}
