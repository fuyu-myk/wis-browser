/**
 * Chrome-side orchestration for the open-in-app prompt. Rust cancels a
 * custom-scheme navigation and emits `wis-app-launch-request`; this reveals
 * the `overlay-applaunch` webview anchored to the top-right of the content
 * host and forwards the payload to the surface via `wis-applaunch-show`.
 * Requests queue one prompt at a time.
 */

import { useEffect, useRef, type RefObject } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { useOverlay } from '@/hooks/useOverlay'

const REQUEST_EVENT = 'wis-app-launch-request'
const SHOW_EVENT = 'wis-applaunch-show'
const CLOSE_EVENT = 'wis-applaunch-close'
const READY_EVENT = 'wis-applaunch-ready'
const PERM_SHOW_EVENT = 'wis-permission-show'
const PERM_CLOSE_EVENT = 'wis-permission-close'

const CARD_W = 340
const CARD_H = 196
const MARGIN = 14
const PERM_SLOT_H = 176

export interface AppLaunchRequest {
  requestId: number
  url: string
  scheme: string
  initiatorHost: string | null
  appName: string | null
  hasHandler: boolean
}

export function useAppLaunchOverlay(hostRef: RefObject<HTMLElement>): void {
  const overlay = useOverlay('applaunch')
  const queueRef = useRef<AppLaunchRequest[]>([])
  const showingRef = useRef(false)
  const permVisibleRef = useRef(false)

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []

    const rectFor = () => {
      const el = hostRef.current
      if (!el) return null
      const r = el.getBoundingClientRect()
      return {
        x: r.x + r.width - CARD_W - MARGIN,
        y: r.y + MARGIN + (permVisibleRef.current ? PERM_SLOT_H : 0),
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

    void listen<AppLaunchRequest>(REQUEST_EVENT, (e) => {
      if (!queueRef.current.some((r) => r.requestId === e.payload.requestId)) {
        queueRef.current.push(e.payload)
      }
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

    const reanchor = () => {
      if (!showingRef.current) return
      const rect = rectFor()
      if (rect) void overlay.show(rect)
    }

    void listen<void>(PERM_SHOW_EVENT, () => {
      permVisibleRef.current = true
      reanchor()
    }).then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    void listen<void>(PERM_CLOSE_EVENT, () => {
      permVisibleRef.current = false
      reanchor()
    }).then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    window.addEventListener('resize', reanchor)

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
      window.removeEventListener('resize', reanchor)
    }
  }, [overlay, hostRef])
}
