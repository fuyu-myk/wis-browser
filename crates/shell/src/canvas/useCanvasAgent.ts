import { useCallback, useEffect, useRef, useState, type MutableRefObject } from 'react'

import type { WireMessage } from '@/lib/ai'
import type { CanvasDoc } from '@/lib/canvas'
import { estimateHeight, runTurn, type AgentContext, type AgentReport, type TurnResult } from '@/lib/canvasAgent'
import type { Attachment } from '@/lib/canvasAttach'
import type { CanvasAiStatus, CanvasChannel, CanvasModels } from '@/lib/canvasChannel'

import type { SurfaceHandle } from './CanvasSurface'

const MAX_HISTORY = 20

export type LogEntry =
  | { id: number; kind: 'user'; text: string; files: readonly string[] }
  | { id: number; kind: 'reply'; text: string }
  | { id: number; kind: 'step'; text: string; details: readonly string[] }
  | { id: number; kind: 'error'; text: string }
  | { id: number; kind: 'think'; text: string; ms: number | null; live: boolean }

type NewEntry = LogEntry extends infer E ? (E extends LogEntry ? Omit<E, 'id'> : never) : never

interface LogEdits {
  add: (entry: NewEntry) => number
  patch: (id: number, changes: Partial<{ text: string; ms: number | null; live: boolean }>) => void
  remove: (id: number) => void
  working: () => void
}

/** Turns a turn's reports into log rows: a live thinking row per round, the
 *  streamed reply, and each step with the cards it touched. */
function logWriter(edits: LogEdits) {
  let reply: number | null = null
  let think: { id: number; started: number; text: string } | null = null
  const settle = () => {
    if (!think) return
    if (think.text === '') edits.remove(think.id)
    else edits.patch(think.id, { ms: performance.now() - think.started, live: false })
    think = null
  }
  const report = (entry: AgentReport) => {
    if (entry.type === 'round') {
      settle()
      think = { id: edits.add({ kind: 'think', text: '', ms: null, live: true }), started: performance.now(), text: '' }
    } else if (entry.type === 'think') {
      if (think) {
        think.text = entry.text
        edits.patch(think.id, { text: entry.text })
      }
    } else if (entry.type === 'working') {
      settle()
      if (reply !== null) edits.remove(reply)
      reply = null
      edits.working()
    } else if (entry.type === 'step') {
      edits.add({ kind: 'step', text: entry.text, details: entry.details })
    } else {
      settle()
      if (reply === null) reply = edits.add({ kind: 'reply', text: entry.text })
      else edits.patch(reply, { text: entry.text })
    }
  }
  return { report, settle }
}

export type AgentActivity = 'idle' | 'thinking' | 'working'

export interface CanvasAgent {
  status: CanvasAiStatus | null
  activity: AgentActivity
  log: readonly LogEntry[]
  send: (prompt: string, files?: readonly Attachment[]) => void
  stop: () => void
  listModels: () => Promise<CanvasModels>
  setModel: (model: string | null) => Promise<void>
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

  const add = (entry: NewEntry) => {
    const id = nextId.current++
    setLog((current) => [...current, { ...entry, id } as LogEntry])
    return id
  }
  const edits: LogEdits = {
    add,
    patch: (id, changes) => setLog((current) => current.map((e) => (e.id === id ? ({ ...e, ...changes } as LogEntry) : e))),
    remove: (id) => setLog((current) => current.filter((e) => e.id !== id)),
    working: () => setActivity('working'),
  }

  const send = (prompt: string, files: readonly Attachment[] = []) => {
    const text = prompt.trim()
    const doc = docRef.current
    if (text === '' || activity !== 'idle' || !doc) return
    stopped.current = false
    setActivity('thinking')
    add({ kind: 'user', text, files: files.map((f) => f.name) })
    const writer = logWriter(edits)
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
        if (entry.type === 'round') setActivity('thinking')
        writer.report(entry)
      },
    }, files)
      .catch((error: unknown): TurnResult => ({
        reply: '',
        created: [],
        error: error instanceof Error ? error.message : 'The agent hit a problem.',
      }))
      .then((result) => {
      request.current = null
      halt.current = () => undefined
      if (!alive.current) return
      writer.settle()
      setActivity('idle')
      if (result.error) add({ kind: 'error', text: result.error })
      else if (result.stopped) add({ kind: 'step', text: 'Stopped', details: [] })
      else if (result.reply.trim() === '') add({ kind: 'step', text: 'Finished', details: [] })
      if (!result.error && !result.stopped) {
        history.current = [
          ...history.current,
          {
            role: 'user' as const,
            content:
              files.length > 0
                ? `${text}\n(The user attached ${files.map((f) => f.name).join(', ')} to this message; their contents are not repeated here.)`
                : text,
          },
          { role: 'assistant' as const, content: result.reply || 'Done.' },
        ].slice(-MAX_HISTORY)
      }
      if (result.created.length > 0) surface.current?.select(result.created)
    })
  }

  const setModel = async (model: string | null) => {
    await channel.aiSetModel(model)
    refresh()
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

  return { status, activity, log, send, stop, clear, refresh, listModels: channel.aiModels, setModel }
}
