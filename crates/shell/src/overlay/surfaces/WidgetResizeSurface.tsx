/**
 * Drag grip for resizing the widget panel. Lives in its own tiny overlay
 * webview straddling the panel's inner edge, because overlays are the only
 * chrome UI promoted above native webviews (the panel and tab webviews would
 * swallow a plain DOM handle's pointer events).
 *
 * The surface only reports horizontal drag deltas; the main chrome owns the
 * geometry (it knows the panel side and width) and resizes the panel live.
 * The overlay webview itself is not moved during the drag so the local
 * coordinate frame stays stable; the chrome repositions it on drag end.
 */

import { useRef } from 'react'
import { emit } from '@tauri-apps/api/event'

import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import {
  WRESIZE_END_EVENT,
  WRESIZE_MOVE_EVENT,
  WRESIZE_START_EVENT,
} from '@/lib/widgetResize'

export function WidgetResizeSurface() {
  useBrowserStateEvents()
  const { data: state } = useBrowserState()
  useThemeApply(state?.settings)

  const draggingRef = useRef(false)
  const startXRef = useRef(0)
  const lastDxRef = useRef(0)
  const rafRef = useRef<number | null>(null)

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return
    e.preventDefault()
    e.currentTarget.setPointerCapture(e.pointerId)
    draggingRef.current = true
    startXRef.current = e.clientX
    lastDxRef.current = 0
    void emit(WRESIZE_START_EVENT).catch(() => undefined)
  }

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!draggingRef.current) return
    lastDxRef.current = e.clientX - startXRef.current
    if (rafRef.current !== null) return
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null
      void emit(WRESIZE_MOVE_EVENT, lastDxRef.current).catch(() => undefined)
    })
  }

  const onPointerUp = () => {
    if (!draggingRef.current) return
    draggingRef.current = false
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current)
      rafRef.current = null
    }
    void emit(WRESIZE_END_EVENT, lastDxRef.current).catch(() => undefined)
  }

  return (
    <div
      className="kb-wresize-surface"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize widget panel"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
    />
  )
}
