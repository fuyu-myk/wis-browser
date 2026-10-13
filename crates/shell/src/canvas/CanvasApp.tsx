import { useCallback, useEffect, useRef, useState } from 'react'

import { parseDoc, serializeDoc, tidyName, type CanvasDoc, type CanvasSummary } from '@/lib/canvas'
import type { CanvasChannel, LoadedCanvas } from '@/lib/canvasChannel'

import { CanvasSurface } from './CanvasSurface'
import { CanvasSwitcher } from './CanvasSwitcher'

/** Rust keeps each save in memory and writes it to disk every few minutes. */
const SAVE_DELAY_MS = 500

interface OpenCanvas {
  id: string
  name: string
  doc: CanvasDoc
  canvases: CanvasSummary[]
}

type State =
  | { kind: 'loading' }
  | { kind: 'error'; message: string }
  | ({ kind: 'ready' } & OpenCanvas)

const messageOf = (error: unknown, fallback: string) =>
  error instanceof Error ? error.message : fallback

/** The canvas in the address, else the most recent, else a new one. */
async function firstCanvas(channel: CanvasChannel): Promise<LoadedCanvas> {
  const wanted = new URLSearchParams(window.location.search).get('canvas')
  const canvases = await channel.list()
  const pick = canvases.find((c) => c.id === wanted) ?? canvases[0] ?? (await channel.create())
  return channel.load(pick.id)
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
      (error: unknown) => setNotice(messageOf(error, 'The canvas could not be saved.')),
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

  /** Drop an unsaved change, for a canvas about to be deleted. */
  const discard = useCallback(() => {
    window.clearTimeout(timer.current)
    pending.current = null
  }, [])

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

  return { save, flush, discard, notice }
}

export function CanvasApp({ channel }: { channel: CanvasChannel }) {
  const [state, setState] = useState<State>({ kind: 'loading' })
  const [notice, setNotice] = useState<string | null>(null)
  const saver = useCanvasSaver(channel, state.kind === 'ready' ? state.id : null)
  const { save, flush, discard } = saver
  const openId = useRef<string | null>(null)
  /** Each switch takes a ticket; only the latest one may show its canvas. */
  const ticket = useRef(0)
  const creating = useRef(false)

  const show = useCallback(
    async (loaded: LoadedCanvas, mine: number) => {
      const canvases = await channel.list()
      if (mine !== ticket.current) return
      openId.current = loaded.id
      document.title = loaded.name
      window.history.replaceState(null, '', `?canvas=${encodeURIComponent(loaded.id)}`)
      setState({ kind: 'ready', id: loaded.id, name: loaded.name, doc: parseDoc(loaded.doc), canvases })
    },
    [channel],
  )

  const attempt = useCallback((work: Promise<unknown>, fallback: string) => {
    work.then(
      () => setNotice(null),
      (error: unknown) => setNotice(messageOf(error, fallback)),
    )
  }, [])

  useEffect(() => {
    let alive = true
    const mine = ++ticket.current
    firstCanvas(channel)
      .then((loaded) => (alive ? show(loaded, mine) : undefined))
      .catch((error: unknown) => {
        if (alive) setState({ kind: 'error', message: messageOf(error, 'The canvas could not be opened.') })
      })
    return () => {
      alive = false
    }
  }, [channel, show])

  const onChange = useCallback(
    (doc: CanvasDoc) => {
      setState((s) => (s.kind === 'ready' ? { ...s, doc } : s))
      save(doc)
    },
    [save],
  )

  const refreshList = () =>
    attempt(
      channel.list().then((canvases) => setState((s) => (s.kind === 'ready' ? { ...s, canvases } : s))),
      'The canvases could not be listed.',
    )
  const open = (id: string) => {
    flush()
    const mine = ++ticket.current
    attempt(
      channel.load(id).then((loaded) => show(loaded, mine)),
      'That canvas could not be opened.',
    )
  }
  const create = () => {
    if (creating.current) return
    creating.current = true
    flush()
    const mine = ++ticket.current
    attempt(
      channel
        .create()
        .then((made) => channel.load(made.id))
        .then((loaded) => show(loaded, mine))
        .finally(() => {
          creating.current = false
        }),
      'A canvas could not be made.',
    )
  }
  /** Show a canvas under `name` at once; Rust's answer confirms or reverts it. */
  const named = (id: string, name: string) => {
    if (openId.current === id) document.title = name
    setState((s) =>
      s.kind === 'ready' && s.id === id
        ? { ...s, name, canvases: s.canvases.map((c) => (c.id === id ? { ...c, name } : c)) }
        : s,
    )
  }
  const rename = (raw: string) => {
    if (state.kind !== 'ready') return
    const { id, name: before } = state
    const name = tidyName(raw)
    if (name === '' || name === before) return
    named(id, name)
    attempt(
      channel.rename(id, name).then(
        (renamed) => named(id, renamed.name),
        (error: unknown) => {
          named(id, before)
          throw error
        },
      ),
      'The canvas could not be renamed.',
    )
  }
  const remove = () => {
    if (state.kind !== 'ready') return
    const mine = ++ticket.current
    attempt(
      channel
        .remove(state.id)
        .then(() => {
          discard()
          return channel.list()
        })
        .then((left) => (left[0] ? left[0] : channel.create()))
        .then((next) => channel.load(next.id))
        .then((loaded) => show(loaded, mine)),
      'The canvas could not be deleted.',
    )
  }

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
  const shownNotice = notice ?? saver.notice
  return (
    <div className="canvas-root">
      <CanvasSurface key={state.id} doc={state.doc} onChange={onChange} />
      <CanvasSwitcher
        name={state.name}
        currentId={state.id}
        canvases={state.canvases}
        onList={refreshList}
        onOpen={open}
        onCreate={create}
        onRename={rename}
        onDelete={remove}
      />
      {shownNotice !== null && (
        <p className="canvas-notice" role="status">
          {shownNotice}
        </p>
      )}
    </div>
  )
}
