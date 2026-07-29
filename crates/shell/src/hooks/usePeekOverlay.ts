/**
 * Chrome-side orchestration for the "Peek" overlay. The native Opt+click
 * handler emits `wis-peek-open`; this hook reveals the full-window `peek`
 * overlay and forwards the link to the surface via `wis-peek-show` (emitted
 * twice to cover the first-open race where the overlay webview was just
 * created). It hides the overlay on `wis-peek-close` and keeps it full-window
 * on resize.
 */

import { useEffect, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { useOverlay } from '@/hooks/useOverlay'
import { setPeekOpen } from '@/lib/peekState'
import { isWindowFocused } from '@/lib/windowFocus'

const PEEK_OPEN_EVENT = 'wis-peek-open'
const PEEK_SHOW_EVENT = 'wis-peek-show'
const PEEK_CLOSE_EVENT = 'wis-peek-close'
const PEEK_READY_EVENT = 'wis-peek-ready'

interface PeekPayload {
  href: string
  text: string
  /** Click point in the source page's viewport coords (-1 = unknown). */
  x?: number
  y?: number
}

interface PeekShowPayload extends PeekPayload {
  /** Click point translated to window coords for the grow-from-link origin. */
  originX?: number
  originY?: number
}

export function usePeekOverlay(): void {
  const overlay = useOverlay('peek')
  const openRef = useRef(false)
  // Last requested peek, replayed when the surface announces it is ready.
  const pendingRef = useRef<PeekShowPayload | null>(null)

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []

    const fullRect = () => ({
      x: 0,
      y: 0,
      width: window.innerWidth,
      height: window.innerHeight,
    })

    const showFor = async (p: PeekPayload) => {
      // Translate the link's page-viewport click point into window coords
      // for the grow-from-link origin.
      const host = document
        .querySelector('.kb-content-host')
        ?.getBoundingClientRect()
      const shown: PeekShowPayload =
        p.x != null && p.y != null && p.x >= 0 && host
          ? { ...p, originX: host.left + p.x, originY: host.top + p.y }
          : { ...p }
      pendingRef.current = shown
      await overlay.show(fullRect())
      openRef.current = true
      setPeekOpen(true)
      void emit(PEEK_SHOW_EVENT, shown)
    }

    void listen<PeekPayload>(PEEK_OPEN_EVENT, (e) => {
      if (!isWindowFocused()) return
      void showFor(e.payload)
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    // The surface (re)mounted: replay the pending peek so it isn't missed.
    void listen<void>(PEEK_READY_EVENT, () => {
      if (pendingRef.current) void emit(PEEK_SHOW_EVENT, pendingRef.current)
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    void listen<void>(PEEK_CLOSE_EVENT, () => {
      // Broadcast event: without the focus gate, closing a peek in one
      // window would desync every other window's peek state.
      if (!isWindowFocused()) return
      openRef.current = false
      setPeekOpen(false)
      pendingRef.current = null
      void overlay.hide().catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    const onResize = () => {
      if (openRef.current) void overlay.show(fullRect())
    }
    window.addEventListener('resize', onResize)

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
      window.removeEventListener('resize', onResize)
    }
  }, [overlay])
}
