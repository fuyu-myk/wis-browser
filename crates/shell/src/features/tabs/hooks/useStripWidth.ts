import { useLayoutEffect, useState, type RefObject } from 'react'

/** The strip's width, and whether it is mid-resize (transitions pause then). */
export function useStripWidth(stripRef: RefObject<HTMLDivElement | null>): {
  stripWidth: number
  stripResizing: boolean
} {
  const [stripWidth, setStripWidth] = useState(0)
  const [stripResizing, setStripResizing] = useState(false)
  useLayoutEffect(() => {
    const strip = stripRef.current
    if (!strip) return
    let timer: number | null = null
    let prevW = 0
    const update = () => {
      const w = strip.clientWidth
      if (prevW !== 0 && Math.abs(prevW - w) > 0.5) {
        setStripResizing(true)
        if (timer !== null) window.clearTimeout(timer)
        timer = window.setTimeout(() => setStripResizing(false), 160)
      }
      prevW = w
      setStripWidth(w)
    }
    update()
    const ro = new ResizeObserver(update)
    ro.observe(strip)
    return () => {
      ro.disconnect()
      if (timer !== null) window.clearTimeout(timer)
    }
  }, [stripRef])
  return { stripWidth, stripResizing }
}
