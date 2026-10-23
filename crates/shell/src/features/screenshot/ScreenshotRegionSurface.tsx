/**
 * Window-region selection overlay, hosted by the `overlay-screenshot-region`
 * webview that covers the whole window (above the tab webviews, so the dragged
 * rectangle can include the chrome). Dims the window, lets the user drag a
 * selection tinted to the theme accent, and reports the rect (content-area CSS
 * px, top-left origin) back to the chrome, which crops the window composite to
 * it. Escape or a too-small drag cancels.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import {
  SCREENSHOT_REGION_BEGIN_EVENT,
  SCREENSHOT_REGION_READY_EVENT,
  SCREENSHOT_REGION_RESULT_EVENT,
  type RegionRect,
} from '@/features/screenshot/screenshot'

interface Box {
  x: number
  y: number
  w: number
  h: number
}

export function ScreenshotRegionSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)

  const [active, setActive] = useState(false)
  const [box, setBox] = useState<Box | null>(null)
  const startRef = useRef<{ x: number; y: number } | null>(null)
  const draggingRef = useRef(false)

  const finish = useCallback((rect: RegionRect | null) => {
    draggingRef.current = false
    startRef.current = null
    setBox(null)
    setActive(false)
    void emit(SCREENSHOT_REGION_RESULT_EVENT, { rect }).catch(() => undefined)
    void commands.hideOverlay('screenshot-region').catch(() => undefined)
  }, [])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    void listen<void>(SCREENSHOT_REGION_BEGIN_EVENT, () => {
      draggingRef.current = false
      startRef.current = null
      setBox(null)
      setActive(true)
      // Take key focus so Escape reaches us even when triggered from a menu.
      void commands.focusSelf().catch(() => undefined)
    }).then((fn) => {
      unlisten = fn
      void emit(SCREENSHOT_REGION_READY_EVENT).catch(() => undefined)
    })
    return () => unlisten?.()
  }, [])

  useEffect(() => {
    if (!active) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        finish(null)
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [active, finish])

  if (!active) return null

  const clampX = (v: number) => Math.max(0, Math.min(v, window.innerWidth))
  const clampY = (v: number) => Math.max(0, Math.min(v, window.innerHeight))

  return (
    <div
      className="kb-shot-region"
      onPointerDown={(e) => {
        if (e.button !== 0 || draggingRef.current) return
        e.preventDefault()
        // Capture the pointer so a drag that leaves the window still resolves.
        e.currentTarget.setPointerCapture(e.pointerId)
        draggingRef.current = true
        startRef.current = { x: e.clientX, y: e.clientY }
        setBox({ x: e.clientX, y: e.clientY, w: 0, h: 0 })
      }}
      onPointerMove={(e) => {
        const start = startRef.current
        if (!draggingRef.current || !start) return
        const ex = clampX(e.clientX)
        const ey = clampY(e.clientY)
        setBox({
          x: Math.min(start.x, ex),
          y: Math.min(start.y, ey),
          w: Math.abs(start.x - ex),
          h: Math.abs(start.y - ey),
        })
      }}
      onPointerUp={(e) => {
        const start = startRef.current
        if (!draggingRef.current || !start) return
        const ex = clampX(e.clientX)
        const ey = clampY(e.clientY)
        const rect: RegionRect = {
          x: Math.min(start.x, ex),
          y: Math.min(start.y, ey),
          width: Math.abs(start.x - ex),
          height: Math.abs(start.y - ey),
        }
        finish(rect.width < 4 || rect.height < 4 ? null : rect)
      }}
      onPointerCancel={() => {
        if (draggingRef.current) finish(null)
      }}
    >
      {box && (
        <div
          className="kb-shot-region-box"
          style={{ left: box.x, top: box.y, width: box.w, height: box.h }}
        />
      )}
    </div>
  )
}
