import { useEffect, useState, type RefObject } from 'react'

import { PIN_STRIP, pinLayout } from '@/features/settings/lib/pinnedTitle'

export interface PinnedTitle {
  key: number
  text: string
  /** Position within the Settings content wrapper. */
  left: number
  top: number
}

const same = (a: PinnedTitle[], b: PinnedTitle[]) =>
  a.length === b.length &&
  a.every((t, i) => t.key === b[i].key && t.text === b[i].text && t.left === b[i].left && t.top === b[i].top)

/**
 * The section titles drawn over the top strip of the Settings scroll area,
 * where the content fades out.
 */
export function usePinnedTitle(main: RefObject<HTMLElement | null>, ready: boolean): PinnedTitle[] {
  const [pinned, setPinned] = useState<PinnedTitle[]>([])

  useEffect(() => {
    const el = main.current
    const wrap = el?.parentElement
    if (!el || !wrap) return
    let frame = 0
    const update = () => {
      frame = 0
      const box = el.getBoundingClientRect()
      const outer = wrap.getBoundingClientRect()
      const titles = [...el.querySelectorAll<HTMLElement>('.sec > .sec-h')]
      const rects = titles.map((title) => title.getBoundingClientRect())
      const layout = pinLayout(
        rects.map((rect) => rect.top - box.top),
        rects.map((rect) => rect.height),
        PIN_STRIP,
      )
      const next = layout.map(({ index, top }) => ({
        key: index,
        text: titles[index].textContent ?? '',
        left: rects[index].left - outer.left,
        top: top + box.top - outer.top,
      }))
      setPinned((prev) => (same(prev, next) ? prev : next))
    }
    const schedule = () => {
      if (frame === 0) frame = requestAnimationFrame(update)
    }
    el.addEventListener('scroll', schedule, { passive: true })
    const resize = new ResizeObserver(schedule)
    resize.observe(el)
    update()
    return () => {
      el.removeEventListener('scroll', schedule)
      resize.disconnect()
      if (frame !== 0) cancelAnimationFrame(frame)
    }
  }, [main, ready])

  return pinned
}
