/**
 * Chrome-side orchestration for the sign-in prompt. Rust holds a site's
 * credential challenge and emits `wis-http-auth-request` for one window; this
 * reveals the `overlay-httpauth` webview at the top right of the content host
 * and hands the surface the request. Requests queue one prompt at a time, and
 * one whose page went away is withdrawn (`wis-http-auth-dismiss`).
 */

import { useEffect, useRef, type RefObject } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import { useOverlay } from '@/hooks/useOverlay'
import { watchLayout } from '@/lib/layoutWatch'

const REQUEST_EVENT = 'wis-http-auth-request'
export const DISMISS_EVENT = 'wis-http-auth-dismiss'
export const SHOW_EVENT = 'wis-httpauth-show'
export const CLOSE_EVENT = 'wis-httpauth-close'
export const READY_EVENT = 'wis-httpauth-ready'

const CARD_W = 340
const CARD_H = 300
const MARGIN = 14

export interface HttpAuthRequest {
  requestId: number
  window: string
  host: string
  realm: string | null
  /** The page is plain `http:`, so the password travels unencrypted. */
  insecure: boolean
  failed: boolean
}

/** A surface's close or ready signal, scoped to its window. */
export interface HttpAuthSignal {
  window: string
  requestId?: number
}

export function useHttpAuthOverlay(hostRef: RefObject<HTMLElement>): void {
  const overlay = useOverlay('httpauth')
  const queueRef = useRef<HttpAuthRequest[]>([])
  const showingRef = useRef(false)

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []
    const keep = (fn: UnlistenFn) => (cancelled ? fn() : unlistens.push(fn))
    const own = getCurrentWindow().label

    const rectFor = () => {
      const el = hostRef.current
      if (!el) return null
      const r = el.getBoundingClientRect()
      return { x: r.x + r.width - CARD_W - MARGIN, y: r.y + MARGIN, width: CARD_W, height: CARD_H }
    }

    const showNext = async () => {
      const req = queueRef.current[0]
      const rect = req ? rectFor() : null
      if (!req || !rect) {
        showingRef.current = false
        return
      }
      showingRef.current = true
      try {
        await overlay.show(rect)
      } catch {
        void commands.httpAuthRespond(req.requestId, null).catch(() => undefined)
        queueRef.current.shift()
        showingRef.current = false
        if (queueRef.current.length > 0) void showNext()
        return
      }
      void emit(SHOW_EVENT, req)
    }

    const finish = (requestId: number) => {
      const head = queueRef.current[0]
      queueRef.current = queueRef.current.filter((r) => r.requestId !== requestId)
      if (head?.requestId !== requestId) return
      void overlay.hide().catch(() => undefined)
      window.setTimeout(() => {
        if (queueRef.current.length > 0) void showNext()
        else showingRef.current = false
      }, 60)
    }

    void listen<HttpAuthRequest>(REQUEST_EVENT, (e) => {
      if (e.payload.window !== own) return
      if (!queueRef.current.some((r) => r.requestId === e.payload.requestId)) {
        queueRef.current.push(e.payload)
      }
      if (!showingRef.current) void showNext()
    }).then(keep)

    void listen<HttpAuthSignal>(READY_EVENT, (e) => {
      const req = queueRef.current[0]
      if (e.payload.window === own && showingRef.current && req) void emit(SHOW_EVENT, req)
    }).then(keep)

    void listen<HttpAuthSignal>(CLOSE_EVENT, (e) => {
      if (e.payload.window === own && e.payload.requestId !== undefined) finish(e.payload.requestId)
    }).then(keep)

    void listen<number>(DISMISS_EVENT, (e) => finish(e.payload)).then(keep)

    const stopWatching = watchLayout(hostRef.current, () => {
      if (!showingRef.current) {
        if (queueRef.current.length > 0) void showNext()
        return
      }
      const rect = rectFor()
      if (rect) void overlay.show(rect)
    })

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
      stopWatching()
      for (const req of queueRef.current) {
        void commands.httpAuthRespond(req.requestId, null).catch(() => undefined)
      }
      queueRef.current = []
      if (showingRef.current) void overlay.hide().catch(() => undefined)
      showingRef.current = false
    }
  }, [overlay, hostRef])
}
