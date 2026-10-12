import { useRef, useState, type KeyboardEvent } from 'react'

import { stepSlot } from '@/lib/usage'

/**
 * A chart's one tab stop: hover or the arrow keys pick a slot up to `last`,
 * and focus starts on `start`.
 */
export function useSlotCursor(last: number, start: number) {
  const [active, setActive] = useState<number | null>(null)
  const focused = useRef(false)

  return {
    active,
    hover: setActive,
    plotProps: {
      tabIndex: 0,
      onKeyDown: (e: KeyboardEvent<HTMLElement>) => {
        const next = stepSlot(e.key, Math.min(active ?? start, last), last)
        if (next === null) return
        e.preventDefault()
        setActive(next)
      },
      onFocus: () => {
        focused.current = true
        setActive((prev) => prev ?? start)
      },
      onBlur: () => {
        focused.current = false
        setActive(null)
      },
      onPointerLeave: () => {
        if (!focused.current) setActive(null)
      },
    },
  }
}
