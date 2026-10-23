import { useEffect, useRef, useState, type RefObject } from 'react'

const MARGIN = '10%'
const SLEEP_MS = 1500

export interface AwakeCards {
  awake: ReadonlySet<string>
  watch: (el: HTMLElement) => void
  unwatch: (el: HTMLElement) => void
}

/** HTML cards on or near the screen; one that leaves sleeps after a moment. */
export function useAwakeCards(root: RefObject<HTMLElement | null>): AwakeCards {
  const [awake, setAwake] = useState<ReadonlySet<string>>(() => new Set())
  const observer = useRef<IntersectionObserver | null>(null)
  const watched = useRef(new Set<HTMLElement>())
  const sleeping = useRef(new Map<string, number>())
  const settle = useRef((id: string, on: boolean) =>
    setAwake((current) => {
      if (current.has(id) === on) return current
      const next = new Set(current)
      if (on) next.add(id)
      else next.delete(id)
      return next
    }),
  ).current

  useEffect(() => {
    const timers = sleeping.current
    const io = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const id = (entry.target as HTMLElement).dataset.card
          if (!id) continue
          window.clearTimeout(timers.get(id))
          timers.delete(id)
          if (entry.isIntersecting) settle(id, true)
          else
            timers.set(
              id,
              window.setTimeout(() => {
                timers.delete(id)
                settle(id, false)
              }, SLEEP_MS),
            )
        }
      },
      { root: root.current, rootMargin: MARGIN },
    )
    observer.current = io
    for (const el of watched.current) io.observe(el)
    return () => {
      io.disconnect()
      observer.current = null
      for (const timer of timers.values()) window.clearTimeout(timer)
      timers.clear()
    }
  }, [root, settle])

  const api = useRef({
    watch(el: HTMLElement) {
      watched.current.add(el)
      observer.current?.observe(el)
    },
    unwatch(el: HTMLElement) {
      watched.current.delete(el)
      observer.current?.unobserve(el)
      const id = el.dataset.card
      if (!id) return
      window.clearTimeout(sleeping.current.get(id))
      sleeping.current.delete(id)
      settle(id, false)
    },
  }).current

  return { awake, watch: api.watch, unwatch: api.unwatch }
}
