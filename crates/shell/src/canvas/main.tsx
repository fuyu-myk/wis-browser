import { createRoot } from 'react-dom/client'

import { readGround, type Ground } from '@/lib/canvasGround'
import { followPushed } from '@/lib/canvasPushed'
import { createCanvasChannel, sealMessage } from '@/lib/canvasChannel'

import { CanvasApp } from './CanvasApp'
import './canvas.css'

declare global {
  interface Window {
    webkit?: { messageHandlers?: { wisCanvas?: Handler } }
    __wisCanvasToken?: unknown
    __wisCanvas?: {
      reply: (raw: unknown) => void
      ai: (requestId: unknown, event: unknown) => void
      mic: (event: unknown) => void
    }
    __wisSwipeAccent?: unknown
    __wisCanvasTheme?: unknown
    __wisCanvasGround?: unknown
  }
}

/** Paint the canvas inside its pane when the tab reaches below it. */
function standOn({ bottom, radius }: Ground) {
  const root = document.documentElement
  root.style.setProperty('--ground', `${bottom}px`)
  root.style.setProperty('--ground-radius', `${radius}px`)
  if (bottom > 0) root.dataset.ground = ''
  else delete root.dataset.ground
}

followPushed(
  window,
  '__wisSwipeAccent',
  (color) => document.documentElement.style.setProperty('--accent', color),
  (value) => CSS.supports('color', value),
)
followPushed(
  window,
  '__wisCanvasTheme',
  (theme) => {
    document.documentElement.dataset.theme = theme
  },
  (value) => value === 'dark' || value === 'light',
)
followPushed(
  window,
  '__wisCanvasSide',
  (side) => {
    document.documentElement.dataset.side = side
  },
  (value) => value === 'left' || value === 'right',
)
followPushed(
  window,
  '__wisCanvasGround',
  (value) => {
    const ground = readGround(value)
    if (ground) standOn(ground)
  },
  (value) => readGround(value) !== null,
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

const token = typeof window.__wisCanvasToken === 'string' ? window.__wisCanvasToken : null
delete window.__wisCanvasToken

void channelHandler().then((handler) => {
  const channel = createCanvasChannel(handler ? (text) => handler.postMessage(sealMessage(token, text)) : null)

  // Rust replies by evaluating `window.__wisCanvas.reply(...)`, streams AI
  // events through `window.__wisCanvas.ai(...)` and dictation through `.mic(...)`.
  Object.defineProperty(window, '__wisCanvas', {
    value: Object.freeze({ reply: channel.reply, ai: channel.ai, mic: channel.mic }),
    writable: false,
    configurable: false,
  })

  void channel.ground().then(
    (value) => {
      const ground = readGround(value)
      if (ground) standOn(ground)
    },
    () => undefined,
  )

  const root = document.getElementById('root')
  if (root) createRoot(root).render(<CanvasApp channel={channel} />)
})
