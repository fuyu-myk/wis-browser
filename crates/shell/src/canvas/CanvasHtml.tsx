import { useEffect, useRef } from 'react'

export const CARD_FRAME_PATH = '/frame'

export function isEscapeFrom(e: MessageEvent, frame: HTMLIFrameElement | null | undefined): boolean {
  const data: unknown = e.data
  return (
    !!frame &&
    e.source === frame.contentWindow &&
    typeof data === 'object' &&
    data !== null &&
    (data as { wisCard?: unknown }).wisCard === 'escape'
  )
}

interface HtmlFrameProps {
  html: string
  live: boolean
}

export function HtmlPaused({ title }: { title: string }) {
  return (
    <div className="canvas-html-paused">
      <span className="canvas-html-glyph" aria-hidden>
        {'</>'}
      </span>
      <span className="canvas-html-title">{title}</span>
    </div>
  )
}

const themeNow = () => document.documentElement.dataset.theme ?? 'dark'

export function HtmlFrame({ html, live }: HtmlFrameProps) {
  const ref = useRef<HTMLIFrameElement>(null)
  useEffect(() => {
    if (live) ref.current?.focus()
  }, [live, html])
  useEffect(() => {
    const retheme = new MutationObserver(() => ref.current?.contentWindow?.postMessage({ wisTheme: themeNow() }, '*'))
    retheme.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
    return () => retheme.disconnect()
  }, [])
  return (
    <>
      <iframe
        key={html}
        ref={ref}
        className="canvas-html-frame"
        title="HTML card"
        sandbox="allow-scripts"
        src={CARD_FRAME_PATH}
        tabIndex={live ? 0 : -1}
        onLoad={(e) =>
          e.currentTarget.contentWindow?.postMessage({ html, theme: themeNow() }, '*')
        }
      />
      {!live && <div className="canvas-html-shield" />}
    </>
  )
}
