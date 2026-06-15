import { useCallback, useRef } from 'react'

/**
 * Remembers a scroll container's position across unmount/remount within a
 * session, keyed by a stable string (e.g. a tab id).
 *
 * Chrome pages (Settings, History, Downloads) are React components rendered into
 * the active pane: switching tabs unmounts them, so without this their scroll
 * resets to the top on return. Native webview tabs keep their scroll for free
 * (the webview is only hidden, never destroyed). The store is module-level and
 * in-memory, so positions survive tab switches but not a full reload.
 */
interface ScrollPos {
  top: number
  left: number
}

// Keyed by the caller's key (tab id). Bounded loosely by the number of chrome
// tabs opened in a session.
const store = new Map<string, ScrollPos>()

/**
 * Returns a callback ref to attach to the scroll container. The position is
 * restored when the node mounts and saved on scroll (throttled to a frame) and
 * on unmount. Using a callback ref (rather than a `useRef` + effect) means
 * containers whose content arrives asynchronously (History/Downloads lists)
 * still restore correctly: the callback fires when the node actually mounts.
 */
export function useScrollMemory(key: string): (el: HTMLElement | null) => void {
  // Track the latest key without re-creating the ref callback.
  const keyRef = useRef(key)
  keyRef.current = key
  const cleanupRef = useRef<(() => void) | null>(null)

  return useCallback((el: HTMLElement | null) => {
    // Detaching the previous node: run its cleanup (saves the final position).
    if (cleanupRef.current) {
      cleanupRef.current()
      cleanupRef.current = null
    }
    if (!el) return

    const saved = store.get(keyRef.current)
    if (saved) {
      el.scrollTop = saved.top
      el.scrollLeft = saved.left
    }

    let raf = 0
    const onScroll = () => {
      if (raf) return
      raf = requestAnimationFrame(() => {
        raf = 0
        store.set(keyRef.current, { top: el.scrollTop, left: el.scrollLeft })
      })
    }
    el.addEventListener('scroll', onScroll, { passive: true })

    cleanupRef.current = () => {
      el.removeEventListener('scroll', onScroll)
      if (raf) cancelAnimationFrame(raf)
      store.set(keyRef.current, { top: el.scrollTop, left: el.scrollLeft })
    }
  }, [])
}
