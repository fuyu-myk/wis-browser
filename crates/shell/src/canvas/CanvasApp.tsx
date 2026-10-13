import { useCallback, useEffect, useRef, useState } from 'react'

import { parseDoc, serializeDoc, type CanvasDoc } from '@/lib/canvas'
import type { CanvasChannel } from '@/lib/canvasChannel'

import { CanvasSurface } from './CanvasSurface'

/** Rust keeps each save in memory and writes it to disk every few minutes. */
const SAVE_DELAY_MS = 500

interface OpenCanvas {
  id: string
  name: string
  doc: CanvasDoc
}

type State =
  | { kind: 'loading' }
  | { kind: 'error'; message: string }
  | ({ kind: 'ready' } & OpenCanvas)

/** The canvas in the address, else the most recent, else a new one. */
async function openCanvas(channel: CanvasChannel): Promise<OpenCanvas> {
  const wanted = new URLSearchParams(window.location.search).get('canvas')
  const canvases = await channel.list()
  const pick = canvases.find((b) => b.id === wanted) ?? canvases[0] ?? (await channel.create())
  const loaded = await channel.load(pick.id)
  return { id: loaded.id, name: loaded.name, doc: parseDoc(loaded.doc) }
}

/** Saves shortly after each change, and at once when the page goes away. */
function useCanvasSaver(channel: CanvasChannel, canvasId: string | null) {
  const pending = useRef<string | null>(null)
  const timer = useRef(0)
  const [notice, setNotice] = useState<string | null>(null)

  const flush = useCallback(() => {
    window.clearTimeout(timer.current)
    const text = pending.current
    if (text === null || canvasId === null) return
    pending.current = null
    channel.save(canvasId, text).then(
      () => setNotice(null),
      (error: unknown) => setNotice(error instanceof Error ? error.message : 'The canvas could not be saved.'),
    )
  }, [channel, canvasId])

  const save = useCallback(
    (doc: CanvasDoc) => {
      pending.current = serializeDoc(doc)
      window.clearTimeout(timer.current)
      timer.current = window.setTimeout(flush, SAVE_DELAY_MS)
    },
    [flush],
  )

  useEffect(() => {
    const onHidden = () => {
      if (document.visibilityState === 'hidden') flush()
    }
    window.addEventListener('pagehide', flush)
    document.addEventListener('visibilitychange', onHidden)
    return () => {
      window.removeEventListener('pagehide', flush)
      document.removeEventListener('visibilitychange', onHidden)
      flush()
    }
  }, [flush])

  return { save, notice }
}

export function CanvasApp({ channel }: { channel: CanvasChannel }) {
  const [state, setState] = useState<State>({ kind: 'loading' })
  const saver = useCanvasSaver(channel, state.kind === 'ready' ? state.id : null)

  useEffect(() => {
    let alive = true
    openCanvas(channel).then(
      (canvas) => {
        if (!alive) return
        document.title = canvas.name
        window.history.replaceState(null, '', `?canvas=${encodeURIComponent(canvas.id)}`)
        setState({ kind: 'ready', ...canvas })
      },
      (error: unknown) => {
        if (alive) {
          setState({
            kind: 'error',
            message: error instanceof Error ? error.message : 'The canvas could not be opened.',
          })
        }
      },
    )
    return () => {
      alive = false
    }
  }, [channel])

  const { save } = saver
  const onChange = useCallback(
    (doc: CanvasDoc) => {
      setState((s) => (s.kind === 'ready' ? { ...s, doc } : s))
      save(doc)
    },
    [save],
  )

  if (state.kind === 'loading') return <div className="canvas-root" />
  if (state.kind === 'error') {
    return (
      <div className="canvas-root">
        <p className="canvas-error" role="alert">
          {state.message}
        </p>
      </div>
    )
  }
  return (
    <div className="canvas-root">
      <CanvasSurface key={state.id} doc={state.doc} onChange={onChange} />
      <div className="canvas-name">{state.name}</div>
      {saver.notice !== null && (
        <p className="canvas-notice" role="status">
          {saver.notice}
        </p>
      )}
    </div>
  )
}
