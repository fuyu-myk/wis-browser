import { useCallback, useEffect, useRef, useState, type MutableRefObject } from 'react'

import type { WireMessage } from '@/lib/ai'
import type { CanvasDoc } from '@/lib/canvas'
import { estimateHeight, runTurn, type AgentContext, type TurnResult } from '@/lib/canvasAgent'
import type { CanvasAiStatus, CanvasChannel } from '@/lib/canvasChannel'

import type { SurfaceHandle } from './CanvasSurface'

const MAX_HISTORY = 20

export type LogEntry =
  | { id: number; kind: 'user'; text: string }
  | { id: number; kind: 'reply'; text: string }
  | { id: number; kind: 'step'; text: string }
  | { id: number; kind: 'error'; text: string }

export type AgentActivity = 'idle' | 'thinking' | 'working'

export interface CanvasAgent {
  status: CanvasAiStatus | null
  activity: AgentActivity
  log: readonly LogEntry[]
  send: (prompt: string) => void
  stop: () => void
  clear: () => void
  refresh: () => void
}

export interface AgentOptions {
  channel: CanvasChannel
  docRef: MutableRefObject<CanvasDoc | null>
  surface: MutableRefObject<SurfaceHandle | null>
  /** Applies an agent change; `first` opens the turn's undo step. */
  apply: (doc: CanvasDoc, first: boolean) => void
}

export function useCanvasAgent({ channel, docRef, surface, apply }: AgentOptions): CanvasAgent {
  const [status, setStatus] = useState<CanvasAiStatus | null>(null)
  const [activity, setActivity] = useState<AgentActivity>('idle')
  const [log, setLog] = useState<LogEntry[]>([])
  const history = useRef<WireMessage[]>([])
  const nextId = useRef(1)
  const request = useRef<string | null>(null)
  const stopped = useRef(false)
  const halt = useRef<() => void>(() => undefined)
  const alive = useRef(true)

  const refresh = useCallback(() => {
    channel.aiStatus().then(
      (next) => alive.current && setStatus(next),
      () => alive.current && setStatus({ available: false, model: '' }),
    )
  }, [channel])

  useEffect(() => {
    alive.current = true
    refresh()
    return () => {
      alive.current = false
      if (request.current) void channel.aiCancel(request.current).catch(() => null)
    }
  }, [channel, refresh])

  const add = (entry: Omit<LogEntry, 'id'>) => {
    const id = nextId.current++
    setLog((current) => [...current, { ...entry, id } as LogEntry])
    return id
  }

  const send = (prompt: string) => {
    const text = prompt.trim()
    const doc = docRef.current
    if (text === '' || activity !== 'idle' || !doc) return
    stopped.current = false
    setActivity('thinking')
    add({ kind: 'user', text })
    let reply: number | null = null
    let changed = false
    const context = (): AgentContext => {
      const s = surface.current
      return {
        selection: s?.selection() ?? [],
        view: s?.view() ?? { x: 0, y: 0, w: window.innerWidth, h: window.innerHeight },
        heightOf: (card) => s?.heightOf(card) ?? estimateHeight(card),
      }
    }
    const halted = new Promise<null>((resolve) => {
      halt.current = () => resolve(null)
    })
    runTurn(history.current, text, {
      chat: (chat, onEvent) => Promise.race([channel.aiChat(chat, onEvent), halted]),
      newRequestId: () => crypto.randomUUID(),
      onRequest: (id) => {
        request.current = id
      },
      getDoc: () => docRef.current ?? doc,
      apply: (next) => {
        apply(next, !changed)
        changed = true
      },
      context,
      cancelled: () => stopped.current || !alive.current,
      report: (entry) => {
        if (!alive.current) return
        if (entry.type === 'working') {
          const drop = reply
          reply = null
          setActivity('working')
          if (drop !== null) setLog((current) => current.filter((e) => e.id !== drop))
        } else if (entry.type === 'step') {
          add({ kind: 'step', text: entry.text })
        } else if (reply === null) {
          reply = add({ kind: 'reply', text: entry.text })
        } else {
          const id = reply
          setLog((current) => current.map((e) => (e.id === id ? { ...e, text: entry.text } : e)))
        }
      },
    })
      .catch((error: unknown): TurnResult => ({
        reply: '',
        created: [],
        error: error instanceof Error ? error.message : 'The agent hit a problem.',
      }))
      .then((result) => {
      request.current = null
      halt.current = () => undefined
      if (!alive.current) return
      setActivity('idle')
      if (result.error) add({ kind: 'error', text: result.error })
      else if (result.stopped) add({ kind: 'step', text: 'Stopped' })
      else if (result.reply.trim() === '') add({ kind: 'step', text: 'Finished' })
      if (!result.error && !result.stopped) {
        history.current = [
          ...history.current,
          { role: 'user' as const, content: text },
          { role: 'assistant' as const, content: result.reply || 'Done.' },
        ].slice(-MAX_HISTORY)
      }
      if (result.created.length > 0) surface.current?.select(result.created)
    })
  }

  const stop = () => {
    stopped.current = true
    if (request.current) void channel.aiCancel(request.current).catch(() => null)
    halt.current()
  }

  const clear = () => {
    if (activity !== 'idle') return
    history.current = []
    setLog([])
  }

  return { status, activity, log, send, stop, clear, refresh }
}
