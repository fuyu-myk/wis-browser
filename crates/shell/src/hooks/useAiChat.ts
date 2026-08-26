import { useCallback, useEffect, useRef, useState } from 'react'
import { Channel } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { StreamEvent, TabId, WorkspaceId } from '@/bindings/types'
import {
  appendDelta,
  hostOf,
  settleTurn,
  wireMessages,
  type AiTurn,
  type SharedPage,
} from '@/lib/ai'

/** Tauri event carrying a tab's extracted readable text. */
const PAGE_TEXT_EVENT = 'wis-page-text'

interface PageTextPayload {
  /** The answering tab, resolved natively; the event is broadcast to every window. */
  tabId: TabId
  url: string
  title: string
  text: string
}

/**
 * How long to wait for a page to answer an extraction request.
 */
const PAGE_TEXT_TIMEOUT_MS = 6000

const PAGE_READ_FAILED = 'Could not read this page.'

/** The tab the panel offers to read, or null when there is no readable page. */
export interface AiPageSource {
  id: TabId
  url: string
  title: string
}

export type AiPhase = 'idle' | 'reading' | 'streaming'

export interface AiChat {
  turns: readonly AiTurn[]
  phase: AiPhase
  /** Transient failure from the last attempt, shown beside the composer. */
  notice: string | null
  /**
   * Resolves true once the exchange is in the transcript, false when it never
   * started.
   */
  send: (question: string, page: AiPageSource | null) => Promise<boolean>
  stop: () => void
  reset: () => void
}

function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (
    error !== null &&
    typeof error === 'object' &&
    'message' in error &&
    typeof (error as { message: unknown }).message === 'string'
  ) {
    return (error as { message: string }).message
  }
  return 'unknown error'
}

/**
 * Session-only assistant conversation for one window. The transcript lives in
 * memory for as long as the panel is mounted and is never persisted. It is
 * also dropped when the active workspace changes, so a private workspace's
 * conversation never carries into another.
 *
 * Streaming deltas are coalesced into one state update per frame.
 */
export function useAiChat(workspaceId: WorkspaceId | null): AiChat {
  const [turns, setTurns] = useState<readonly AiTurn[]>([])
  const [phase, setPhase] = useState<AiPhase>('idle')
  const [notice, setNotice] = useState<string | null>(null)

  // Bumped whenever a send is superseded or stopped, so a late delta from an
  // abandoned stream is dropped instead of appended.
  const genRef = useRef(0)
  const nextIdRef = useRef(1)
  const turnsRef = useRef<readonly AiTurn[]>([])
  turnsRef.current = turns

  /** Request id of the stream in flight, so stopping can cancel it upstream. */
  const requestIdRef = useRef<string | null>(null)

  const pendingPageRef = useRef<{
    tabId: TabId
    timer: number
    deliver: (page: SharedPage) => void
    abandon: (reason: Error) => void
  } | null>(null)

  /** Drop an in-flight page read so its awaiting send unwinds. */
  const abandonPageRead = useCallback(() => {
    const pending = pendingPageRef.current
    if (!pending) return
    pendingPageRef.current = null
    window.clearTimeout(pending.timer)
    pending.abandon(new Error(PAGE_READ_FAILED))
  }, [])

  const deltaRef = useRef<{ id: number; content: string; reasoning: string } | null>(null)
  const rafRef = useRef<number | null>(null)

  const applyPending = useCallback(() => {
    const chunk = deltaRef.current
    deltaRef.current = null
    if (chunk) setTurns((prev) => appendDelta(prev, chunk.id, chunk.content, chunk.reasoning))
  }, [])

  const flushDeltas = useCallback(() => {
    if (rafRef.current !== null) {
      window.cancelAnimationFrame(rafRef.current)
      rafRef.current = null
    }
    applyPending()
  }, [applyPending])

  const queueDelta = useCallback(
    (id: number, content: string, reasoning: string) => {
      const pending = deltaRef.current
      deltaRef.current =
        pending !== null && pending.id === id
          ? {
              id,
              content: pending.content + content,
              reasoning: pending.reasoning + reasoning,
            }
          : { id, content, reasoning }
      if (rafRef.current !== null) return
      rafRef.current = window.requestAnimationFrame(() => {
        rafRef.current = null
        applyPending()
      })
    },
    [applyPending],
  )

  useEffect(
    () => () => {
      if (rafRef.current !== null) window.cancelAnimationFrame(rafRef.current)
      const pending = pendingPageRef.current
      if (pending) {
        pendingPageRef.current = null
        window.clearTimeout(pending.timer)
        pending.abandon(new Error(PAGE_READ_FAILED))
      }
    },
    [],
  )

  // Page text is broadcast to every window and carries no request identity, so
  // a response counts only when it comes from the tab this panel asked.
  useEffect(() => {
    let cancelled = false
    let unlisten: UnlistenFn | null = null

    void listen<PageTextPayload>(PAGE_TEXT_EVENT, (e) => {
      const pending = pendingPageRef.current
      if (!pending || pending.tabId !== e.payload.tabId) return
      pendingPageRef.current = null
      window.clearTimeout(pending.timer)
      pending.deliver({ url: e.payload.url, title: e.payload.title, text: e.payload.text })
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })

    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const readPage = useCallback((source: AiPageSource): Promise<SharedPage> => {
    return new Promise<SharedPage>((resolve, reject) => {
      const timer = window.setTimeout(() => {
        if (pendingPageRef.current?.timer !== timer) return
        pendingPageRef.current = null
        reject(new Error(PAGE_READ_FAILED))
      }, PAGE_TEXT_TIMEOUT_MS)

      pendingPageRef.current = {
        tabId: source.id,
        timer,
        deliver: resolve,
        abandon: reject,
      }

      void commands.requestPageText(source.id).catch(() => {
        if (pendingPageRef.current?.timer !== timer) return
        pendingPageRef.current = null
        window.clearTimeout(timer)
        reject(new Error(PAGE_READ_FAILED))
      })
    })
  }, [])

  /** Abandon whatever is in flight, upstream request included. */
  const abandonInFlight = useCallback(() => {
    genRef.current += 1
    abandonPageRead()
    const requestId = requestIdRef.current
    if (requestId !== null) {
      requestIdRef.current = null
      void commands.aiCancel(requestId).catch(() => undefined)
    }
  }, [abandonPageRead])

  const stop = useCallback(() => {
    abandonInFlight()
    flushDeltas()
    setTurns((prev) => prev.map((t) => (t.streaming ? { ...t, streaming: false } : t)))
    setPhase('idle')
  }, [abandonInFlight, flushDeltas])

  const reset = useCallback(() => {
    abandonInFlight()
    deltaRef.current = null
    if (rafRef.current !== null) {
      window.cancelAnimationFrame(rafRef.current)
      rafRef.current = null
    }
    setTurns([])
    setPhase('idle')
    setNotice(null)
  }, [abandonInFlight])

  // A conversation belongs to the workspace it was held in.
  useEffect(() => {
    reset()
  }, [workspaceId, reset])

  const send = useCallback(
    async (question: string, page: AiPageSource | null): Promise<boolean> => {
      const asked = question.trim()
      if (asked === '' || workspaceId === null) return false

      const gen = ++genRef.current
      setNotice(null)

      let shared: SharedPage | null = null
      if (page !== null) {
        setPhase('reading')
        try {
          shared = await readPage(page)
        } catch (error: unknown) {
          if (genRef.current !== gen) return false
          setPhase('idle')
          setNotice(errorMessage(error))
          return false
        }
        if (genRef.current !== gen) return false
      }

      const userId = nextIdRef.current++
      const assistantId = nextIdRef.current++
      const history = turnsRef.current
      const messages = wireMessages(history, asked, shared)
      const requestId = crypto.randomUUID()
      requestIdRef.current = requestId

      setTurns([
        ...history,
        {
          id: userId,
          role: 'user',
          content: asked,
          reasoning: '',
          wire: shared ? messages[messages.length - 1].content : null,
          sharedHost: shared ? hostOf(shared.url) : null,
          reasoningMs: null,
          error: null,
          streaming: false,
        },
        {
          id: assistantId,
          role: 'assistant',
          content: '',
          reasoning: '',
          wire: null,
          sharedHost: null,
          reasoningMs: null,
          error: null,
          streaming: true,
        },
      ])
      setPhase('streaming')

      // The exchange is committed to the transcript from here, so the caller
      // may clear the composer; the stream itself runs on unattended.
      void (async () => {
        let reasonedAt: number | null = null
        let reasonedFor: number | null = null
        const closeReasoning = () => {
          if (reasonedAt === null || reasonedFor !== null) return
          reasonedFor = Date.now() - reasonedAt
          const ms = reasonedFor
          setTurns((prev) =>
            prev.map((t) => (t.id === assistantId ? { ...t, reasoningMs: ms } : t)),
          )
        }

        let settled = false
        const settle = (error: string | null) => {
          if (settled || genRef.current !== gen) return
          settled = true
          if (requestIdRef.current === requestId) requestIdRef.current = null
          flushDeltas()
          closeReasoning()
          setTurns((prev) => settleTurn(prev, assistantId, error))
          setPhase('idle')
        }

        const channel = new Channel<StreamEvent>()
        channel.onmessage = (event) => {
          if (genRef.current !== gen) return
          if (event.type === 'delta') {
            closeReasoning()
            queueDelta(assistantId, event.text, '')
          } else if (event.type === 'reasoning') {
            if (reasonedAt === null) reasonedAt = Date.now()
            queueDelta(assistantId, '', event.text)
          } else if (event.type === 'done') settle(null)
          else settle(event.message)
        }

        try {
          await commands.aiChat(
            { workspace_id: workspaceId, request_id: requestId, messages },
            channel,
          )
          settle(null)
        } catch (error: unknown) {
          settle(errorMessage(error))
        }
      })()

      return true
    },
    [workspaceId, readPage, flushDeltas, queueDelta],
  )

  return { turns, phase, notice, send, stop, reset }
}
