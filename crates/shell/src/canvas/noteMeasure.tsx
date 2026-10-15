import { flushSync } from 'react-dom'
import { createRoot, type Root } from 'react-dom/client'

import { AiMarkdown } from '@/components/AiMarkdown'

const MAX_KEPT = 256
const ignore = () => undefined

/** Measures notes the way the canvas lays them out: a hidden card renders the
 *  Markdown at the given width inside the layer, for one synchronous pass. */
export function noteMeasurer(): (layer: HTMLElement, text: string, w: number) => number {
  let host: HTMLDivElement | null = null
  let root: Root | null = null
  const kept = new Map<string, number>()
  return (layer, text, w) => {
    const key = `${w}:${text}`
    const known = kept.get(key)
    if (known !== undefined) return known
    if (!host || !root) {
      host = document.createElement('div')
      host.className = 'canvas-card canvas-measure'
      host.setAttribute('aria-hidden', 'true')
      root = createRoot(host)
    }
    const card = host
    const view = root
    card.style.width = `${w}px`
    layer.appendChild(card)
    flushSync(() => view.render(<AiMarkdown text={text} onOpenLink={ignore} />))
    const height = card.offsetHeight
    const drawn = card.childElementCount > 0
    card.remove()
    if (!drawn) return height
    const oldest = kept.size >= MAX_KEPT ? kept.keys().next().value : undefined
    if (oldest !== undefined) kept.delete(oldest)
    kept.set(key, height)
    return height
  }
}
