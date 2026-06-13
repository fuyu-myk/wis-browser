/**
 * Chrome-side orchestration for the camera/microphone consent prompt. The
 * native `wisMediaPermission` handler emits `wis-permission-request` when a page
 * asks and the answer isn't remembered; this reveals the `overlay-permission`
 * webview anchored to the top-right of the content host and forwards the payload
 * to the surface via `wis-permission-show`. Requests queue (one prompt at a
 * time); `wis-permission-close` (sent after the user decides) hides the overlay
 * and shows the next. The surface replays the current request on mount via
 * `wis-permission-ready` to cover the first-open race.
 */

import { useEffect, useRef, type RefObject } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { useOverlay } from '@/hooks/useOverlay'

const REQUEST_EVENT = 'wis-permission-request'
const SHOW_EVENT = 'wis-permission-show'
const CLOSE_EVENT = 'wis-permission-close'
const READY_EVENT = 'wis-permission-ready'

const CARD_W = 320
const CARD_H = 132
const MARGIN = 14

interface PermissionRequest {
  request_id: number
  origin: string
  kind: string
}

export function usePermissionOverlay(hostRef: RefObject<HTMLElement>): void {
  const overlay = useOverlay('permission')
  const queueRef = useRef<PermissionRequest[]>([])
  const showingRef = useRef(false)

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []

    const rectFor = () => {
      const el = hostRef.current
      if (!el) return null
      const r = el.getBoundingClientRect()
      return {
        x: r.x + r.width - CARD_W - MARGIN,
        y: r.y + MARGIN,
        width: CARD_W,
        height: CARD_H,
      }
    }

    const showNext = async () => {
      const req = queueRef.current[0]
      const rect = req ? rectFor() : null
      if (!req || !rect) {
        showingRef.current = false
        return
      }
      showingRef.current = true
      await overlay.show(rect)
      void emit(SHOW_EVENT, req)
    }

    void listen<PermissionRequest>(REQUEST_EVENT, (e) => {
      queueRef.current.push(e.payload)
      if (!showingRef.current) void showNext()
    }).then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    void listen<void>(READY_EVENT, () => {
      const req = queueRef.current[0]
      if (showingRef.current && req) void emit(SHOW_EVENT, req)
    }).then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    void listen<void>(CLOSE_EVENT, () => {
      queueRef.current.shift()
      void overlay.hide().catch(() => undefined)
      window.setTimeout(() => {
        if (queueRef.current.length > 0) void showNext()
        else showingRef.current = false
      }, 60)
    }).then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    const onResize = () => {
      if (!showingRef.current) return
      const rect = rectFor()
      if (rect) void overlay.show(rect)
    }
    window.addEventListener('resize', onResize)

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
      window.removeEventListener('resize', onResize)
    }
  }, [overlay, hostRef])
}
