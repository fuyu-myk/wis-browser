import { useEffect, useRef, type MutableRefObject, type RefObject } from 'react'

import { zoomAt, type CanvasView } from '@/features/canvas/lib/canvas'

const LINE_PX = 16

interface GestureEvent extends UIEvent {
  scale: number
  clientX: number
  clientY: number
}

export function useViewGestures(
  viewport: RefObject<HTMLElement | null>,
  view: MutableRefObject<CanvasView>,
  moved: () => void,
): void {
  const onMoved = useRef(moved)
  onMoved.current = moved

  useEffect(() => {
    const el = viewport.current
    if (!el) return
    const local = (clientX: number, clientY: number) => {
      const rect = el.getBoundingClientRect()
      return { x: clientX - rect.left, y: clientY - rect.top }
    }
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && (e.target as Element).closest('textarea')) return
      e.preventDefault()
      const scale = e.deltaMode === 1 ? LINE_PX : 1
      const p = local(e.clientX, e.clientY)
      view.current = e.ctrlKey
        ? zoomAt(view.current, p.x, p.y, Math.exp(-e.deltaY * scale * 0.01))
        : { ...view.current, x: view.current.x - e.deltaX * scale, y: view.current.y - e.deltaY * scale }
      onMoved.current()
    }
    let pinch = 1
    const onGestureStart = (e: Event) => {
      e.preventDefault()
      pinch = 1
    }
    const onGestureChange = (e: Event) => {
      e.preventDefault()
      const g = e as GestureEvent
      const p = local(g.clientX, g.clientY)
      view.current = zoomAt(view.current, p.x, p.y, g.scale / pinch)
      pinch = g.scale
      onMoved.current()
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    el.addEventListener('gesturestart', onGestureStart)
    el.addEventListener('gesturechange', onGestureChange)
    return () => {
      el.removeEventListener('wheel', onWheel)
      el.removeEventListener('gesturestart', onGestureStart)
      el.removeEventListener('gesturechange', onGestureChange)
    }
  }, [viewport, view])
}
