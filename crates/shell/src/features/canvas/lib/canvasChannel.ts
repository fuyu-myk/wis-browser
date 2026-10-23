/**
 * The canvas page's requests to Rust over `wisCanvas`: each message carries an
 * id, and Rust's reply, evaluated into the page, settles the matching request.
 */

import type { AiFetchResponse, AiSearchResult, StreamEvent } from '@/bindings/types'
import type { WireMessage } from '@/features/ai/lib/ai'
import type { CanvasSummary } from '@/features/canvas/lib/canvas'
import type { DocumentKind, DocumentOutcome } from '@/features/canvas/lib/canvasAttach'
import { readMicEvent, type MicEvent } from '@/features/canvas/lib/canvasDictation'

export interface LoadedCanvas {
  id: string
  name: string
  doc: string
  updatedAt: number
}

export interface CanvasAiStatus {
  available: boolean
  model: string
  /** Whether this platform has the agent at all. */
  supported?: boolean
  /** Whether this platform can dictate prompts, and read PDF and Word files. */
  dictation?: boolean
  documents?: boolean
}

export interface CanvasModels {
  models: { id: string; name: string }[]
  /** The canvas's own choice; `null` follows the assistant's model. */
  selected: string | null
  assistant: string
}

export interface CanvasAiChat {
  request_id: string
  messages: WireMessage[]
  skip_reasoning?: boolean
  no_tools?: boolean
}

export interface CanvasChannel {
  reply: (raw: unknown) => void
  /** Rust pushes a streaming AI event for the request it names. */
  ai: (requestId: unknown, event: unknown) => void
  list: () => Promise<CanvasSummary[]>
  load: (canvas: string) => Promise<LoadedCanvas>
  create: (name?: string) => Promise<CanvasSummary>
  rename: (canvas: string, name: string) => Promise<CanvasSummary>
  save: (canvas: string, doc: string) => Promise<null>
  remove: (canvas: string) => Promise<null>
  /** Reports running HTML cards; resolves true when they should start paused. */
  pulse: (canvas: string, state: 'start' | 'beat' | 'stop') => Promise<boolean>
  aiStatus: () => Promise<CanvasAiStatus>
  /** Streams events to `onEvent` until the request settles. */
  aiChat: (chat: CanvasAiChat, onEvent: (event: StreamEvent) => void) => Promise<null>
  aiCancel: (requestId: string) => Promise<null>
  /** How far this tab reaches below its pane (see `features/canvas/lib/canvasGround.ts`). */
  ground: () => Promise<unknown>
  aiModels: () => Promise<CanvasModels>
  aiSetModel: (model: string | null) => Promise<null>
  aiWebSearch: (query: string) => Promise<AiSearchResult[]>
  aiWebFetch: (url: string) => Promise<AiFetchResponse>
  /** Text from a PDF or word processor file, given as base64. */
  aiReadDocument: (kind: DocumentKind, data: string, maxChars: number) => Promise<DocumentOutcome>
  /** Starts on-device dictation; what it hears arrives through `onMic`. */
  aiMicStart: () => Promise<null>
  aiMicStop: () => Promise<null>
  /** Rust pushes what dictation hears. */
  mic: (event: unknown) => void
  onMic: (listener: (event: MicEvent) => void) => () => void
}

interface Waiting {
  resolve: (value: unknown) => void
  reject: (error: Error) => void
}

const UNAVAILABLE = 'Canvases are not available here.'

/** On Linux, where the engine cannot tell frames apart, each message opens
 *  with the tab's token, which only the canvas page holds. */
export function sealMessage(token: string | null, text: string): string {
  return token === null ? text : `${token}\n${text}`
}

export function createCanvasChannel(post: ((text: string) => void) | null): CanvasChannel {
  let next = 1
  const waiting = new Map<number, Waiting>()
  const streams = new Map<string, (event: StreamEvent) => void>()
  const hearing = new Set<(event: MicEvent) => void>()

  function request<T>(message: Record<string, unknown>): Promise<T> {
    if (post === null) return Promise.reject(new Error(UNAVAILABLE))
    const id = next
    next += 1
    return new Promise<T>((resolve, reject) => {
      waiting.set(id, { resolve: (value) => resolve(value as T), reject })
      post(JSON.stringify({ id, ...message }))
    })
  }

  function reply(raw: unknown) {
    if (typeof raw !== 'object' || raw === null) return
    const { id, ok, value, error } = raw as Record<string, unknown>
    if (typeof id !== 'number') return
    const pending = waiting.get(id)
    if (pending === undefined) return
    waiting.delete(id)
    if (ok === true) pending.resolve(value ?? null)
    else pending.reject(new Error(typeof error === 'string' ? error : 'The canvas request failed.'))
  }

  function ai(requestId: unknown, event: unknown) {
    if (typeof requestId !== 'string' || typeof event !== 'object' || event === null) return
    if (typeof (event as { type?: unknown }).type !== 'string') return
    streams.get(requestId)?.(event as StreamEvent)
  }

  return {
    reply,
    ai,
    list: () => request({ op: 'list' }),
    load: (canvas) => request({ op: 'load', canvas }),
    create: (name) => request({ op: 'create', name }),
    rename: (canvas, name) => request({ op: 'rename', canvas, name }),
    save: (canvas, doc) => request({ op: 'save', canvas, doc }),
    remove: (canvas) => request({ op: 'delete', canvas }),
    pulse: (canvas, state) =>
      request<unknown>({ op: 'pulse', canvas, state }).then(
        (value) => typeof value === 'object' && value !== null && (value as { safe?: unknown }).safe === true,
      ),
    aiStatus: () => request({ op: 'aiStatus' }),
    aiChat: (chat, onEvent) => {
      streams.set(chat.request_id, onEvent)
      return request<null>({ op: 'aiChat', ...chat }).finally(() => streams.delete(chat.request_id))
    },
    aiCancel: (requestId) => request({ op: 'aiCancel', request_id: requestId }),
    ground: () => request({ op: 'ground' }),
    aiModels: () => request({ op: 'aiModels' }),
    aiSetModel: (model) => request({ op: 'aiSetModel', model }),
    aiWebSearch: (query) => request({ op: 'aiWebSearch', query }),
    aiWebFetch: (url) => request({ op: 'aiWebFetch', url }),
    aiReadDocument: (kind, data, maxChars) => request({ op: 'aiReadDocument', kind, max_chars: maxChars, data }),
    aiMicStart: () => request({ op: 'aiMicStart' }),
    aiMicStop: () => request({ op: 'aiMicStop' }),
    mic: (raw) => {
      const event = readMicEvent(raw)
      if (event) for (const listener of hearing) listener(event)
    },
    onMic: (listener) => {
      hearing.add(listener)
      return () => hearing.delete(listener)
    },
  }
}
