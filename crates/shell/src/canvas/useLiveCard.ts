import { useEffect, useState, type MutableRefObject, type RefObject } from 'react'

import { isEscapeFrom } from './CanvasHtml'

/** The HTML card taking input; a click outside it or its Escape ends that. */
export function useLiveCard(
  viewport: RefObject<HTMLElement | null>,
  cardEls: MutableRefObject<Map<string, HTMLElement>>,
): [string | null, (id: string | null) => void] {
  const [live, setLive] = useState<string | null>(null)

  useEffect(() => {
    if (live === null) {
      if (document.activeElement instanceof HTMLIFrameElement) viewport.current?.focus({ preventScroll: true })
      return
    }
    const card = () => cardEls.current.get(live)
    const onDown = (e: PointerEvent) => {
      if (!card()?.contains(e.target as Node)) setLive(null)
    }
    const onMessage = (e: MessageEvent) => {
      if (isEscapeFrom(e, card()?.querySelector('iframe'))) setLive(null)
    }
    window.addEventListener('pointerdown', onDown, true)
    window.addEventListener('message', onMessage)
    return () => {
      window.removeEventListener('pointerdown', onDown, true)
      window.removeEventListener('message', onMessage)
    }
  }, [live, viewport, cardEls])

  return [live, setLive]
}
