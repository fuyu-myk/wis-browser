import { createRoot } from 'react-dom/client'

import { followThemeAccent } from '@/lib/canvasAccent'
import { createCanvasChannel } from '@/lib/canvasChannel'

import { CanvasApp } from './CanvasApp'
import './canvas.css'

declare global {
  interface Window {
    webkit?: { messageHandlers?: { wisCanvas?: Handler } }
    __wisCanvas?: { reply: (raw: unknown) => void }
    __wisSwipeAccent?: unknown
  }
}

followThemeAccent(
  window,
  (color) => document.documentElement.style.setProperty('--accent', color),
  (value) => CSS.supports('color', value),
)

type Handler = { postMessage: (text: string) => void }

/** Rust installs the channel just after the tab opens, so the page can start first. */
const HANDLER_WAIT_MS = 3000
const HANDLER_POLL_MS = 25

async function channelHandler(): Promise<Handler | null> {
  for (let waited = 0; waited < HANDLER_WAIT_MS; waited += HANDLER_POLL_MS) {
    const handler = window.webkit?.messageHandlers?.wisCanvas
    if (handler) return handler
    await new Promise((resolve) => setTimeout(resolve, HANDLER_POLL_MS))
  }
  return null
}

void channelHandler().then((handler) => {
  const channel = createCanvasChannel(handler ? (text) => handler.postMessage(text) : null)

  // Rust replies by evaluating `window.__wisCanvas.reply(...)`.
  Object.defineProperty(window, '__wisCanvas', {
    value: Object.freeze({ reply: channel.reply }),
    writable: false,
    configurable: false,
  })

  const root = document.getElementById('root')
  if (root) createRoot(root).render(<CanvasApp channel={channel} />)
})
