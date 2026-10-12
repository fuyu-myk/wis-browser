import { useCallback, useRef, useState, type PointerEvent } from 'react'

/**
 * A docked panel's width, kept in localStorage and resized from a grip on its
 * inner edge. Dragging broadcasts `wis-pane-resize` so page webviews follow.
 */
export function useDockWidth(
  storageKey: string,
  initial: number,
  clamp: (width: number) => number,
  side: 'left' | 'right',
) {
  const [width, setWidth] = useState(() => {
    const raw = window.localStorage.getItem(storageKey)
    const parsed = raw === null ? Number.NaN : Number.parseInt(raw, 10)
    return clamp(Number.isNaN(parsed) ? initial : parsed)
  })
  const widthRef = useRef(width)
  widthRef.current = width

  const startResize = useCallback(
    (e: PointerEvent<HTMLDivElement>) => {
      if (e.button !== 0) return
      e.preventDefault()
      const startX = e.clientX
      const startWidth = widthRef.current
      const setActive = (active: boolean) =>
        window.dispatchEvent(new CustomEvent('wis-pane-resize', { detail: active }))

      const onMove = (ev: globalThis.PointerEvent) => {
        const delta = side === 'right' ? startX - ev.clientX : ev.clientX - startX
        setWidth(clamp(startWidth + delta))
      }
      const onUp = () => {
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
        setActive(false)
        window.localStorage.setItem(storageKey, String(widthRef.current))
      }

      setActive(true)
      window.addEventListener('pointermove', onMove)
      window.addEventListener('pointerup', onUp)
    },
    [side, clamp, storageKey],
  )

  return { width, startResize }
}
