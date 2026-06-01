/**
 * Frontend handle on a named overlay webview. The first call to
 * `show(rect)` lazily creates the webview via `ensureOverlay`; subsequent
 * calls just reposition it. `hide` collapses it via `hideOverlay`.
 *
 * `show` accepts a `DOMRect`-like object so callers can pass any anchor's
 * `getBoundingClientRect()` straight through.
 */

import { useCallback, useRef } from 'react'

import { commands } from '@/bindings/commands'

interface Rect {
  x: number
  y: number
  width: number
  height: number
}

interface UseOverlayReturn {
  show: (rect: Rect) => Promise<void>
  hide: () => Promise<void>
}

export function useOverlay(name: string): UseOverlayReturn {
  const ensuredRef = useRef(false)

  const show = useCallback(
    async (rect: Rect) => {
      if (!ensuredRef.current) {
        await commands.ensureOverlay(name)
        ensuredRef.current = true
      }
      await commands.setOverlayBounds(name, rect.x, rect.y, rect.width, rect.height)
    },
    [name],
  )

  const hide = useCallback(async () => {
    if (!ensuredRef.current) return
    await commands.hideOverlay(name)
  }, [name])

  return { show, hide }
}
