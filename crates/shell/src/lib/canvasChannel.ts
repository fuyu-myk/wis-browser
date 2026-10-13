/**
 * The canvas page's requests to Rust over `wisCanvas`: each message carries an
 * id, and Rust's reply, evaluated into the page, settles the matching request.
 */

import type { CanvasSummary } from '@/lib/canvas'

export interface LoadedCanvas {
  id: string
  name: string
  doc: string
  updatedAt: number
}

export interface CanvasChannel {
  reply: (raw: unknown) => void
  list: () => Promise<CanvasSummary[]>
  load: (canvas: string) => Promise<LoadedCanvas>
  create: (name?: string) => Promise<CanvasSummary>
  rename: (canvas: string, name: string) => Promise<CanvasSummary>
  save: (canvas: string, doc: string) => Promise<null>
  remove: (canvas: string) => Promise<null>
}

interface Waiting {
  resolve: (value: unknown) => void
  reject: (error: Error) => void
}

const UNAVAILABLE = 'Canvases are not available here.'

export function createCanvasChannel(post: ((text: string) => void) | null): CanvasChannel {
  let next = 1
  const waiting = new Map<number, Waiting>()

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

  return {
    reply,
    list: () => request({ op: 'list' }),
    load: (canvas) => request({ op: 'load', canvas }),
    create: (name) => request({ op: 'create', name }),
    rename: (canvas, name) => request({ op: 'rename', canvas, name }),
    save: (canvas, doc) => request({ op: 'save', canvas, doc }),
    remove: (canvas) => request({ op: 'delete', canvas }),
  }
}
