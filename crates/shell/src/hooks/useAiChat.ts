import { useCallback, useEffect, useRef, useState } from 'react'
import { Channel } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { AiToolCall, StreamEvent, TabId, WorkspaceId } from '@/bindings/types'
import {
  ANSWER_NOW_DIRECTIVE,
  appendDelta,
  appendToolSegments,
  cancelRunningTools,
  closeThink,
  extractUrls,
  fetchToolContent,
  finalizeInterrupted,
  hostOf,
  MAX_TOOL_ROUNDS,
  planToolCalls,
  previewToolResponse,
  roundSegments,
  searchToolContent,
  settleToolCall,
  settleTurn,
  TOOL_BUDGET_DIRECTIVE,
  wireMessages,
  type AiTurn,
  type SharedPage,
  type WireMessage,
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
  /**
   * Cut the current thinking or tool round short: cancel whatever is in
   * flight and restart the same turn demanding an immediate answer, tools
   * withheld. Only meaningful while a response is underway.
   */
  answerNow: () => void
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
 *
 * `webEnabled` marks that web tools can actually be offered for the current
 * settings and platform; it steers the system prompt and nothing else.
 */
export function useAiChat(workspaceId: WorkspaceId | null, webEnabled: boolean): AiChat {
  const [turns, setTurns] = useState<readonly AiTurn[]>([])
  const [phase, setPhase] = useState<AiPhase>('idle')
  const [notice, setNotice] = useState<string | null>(null)

  // Bumped whenever a send is superseded or stopped, so a late delta from an
  // abandoned stream, or a late tool result, is dropped instead of applied.
  const genRef = useRef(0)
  const nextIdRef = useRef(1)
  const turnsRef = useRef<readonly AiTurn[]>([])
  turnsRef.current = turns

  /** Request id of the stream in flight, so stopping can cancel it upstream. */
  const requestIdRef = useRef<string | null>(null)
  /** Restarts the in-flight exchange's stream, for the answer-now path. */
  const restartRef = useRef<((gen: number, immediate: boolean) => void) | null>(null)

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
    restartRef.current = null
    const requestId = requestIdRef.current
    if (requestId !== null) {
      requestIdRef.current = null
      void commands.aiCancel(requestId).catch(() => undefined)
    }
  }, [abandonPageRead])

  const answerNow = useCallback(() => {
    const restart = restartRef.current
    if (restart === null) return
    const requestId = requestIdRef.current
    if (requestId !== null) {
      requestIdRef.current = null
      void commands.aiCancel(requestId).catch(() => undefined)
    }
    flushDeltas()
    restart(++genRef.current, true)
  }, [flushDeltas])

  const stop = useCallback(() => {
    abandonInFlight()
    flushDeltas()
    setTurns((prev) => finalizeInterrupted(prev))
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
      const requestWorkspaceId = workspaceId

      // The exchange's wire, grown in place by tool rounds.
      let convo: WireMessage[] = wireMessages(history, asked, shared, webEnabled)
      // Fetches are allowed only for URLs the user wrote or a search returned.
      const allowedUrls = new Set<string>(extractUrls(asked))
      for (const t of history) {
        if (t.role !== 'user') continue
        for (const u of extractUrls(t.content)) allowedUrls.add(u)
      }
      const seenQueries = new Set<string>()
      let rounds = 0
      let toolsExhausted = false
      /** Answer text of the current stream round, for its wire message. */
      let roundText = ''
      /** Sticky across rounds: any truncation marks the whole turn. */
      let truncated = false
      /** When the live thinking spell started, for its duration label. */
      let thinkStart: number | null = null

      setTurns([
        ...history,
        {
          id: userId,
          role: 'user',
          content: asked,
          segments: [],
          wire: shared ? convo[convo.length - 1].content : null,
          sharedHost: shared ? hostOf(shared.url) : null,
          truncated: false,
          error: null,
          streaming: false,
        },
        {
          id: assistantId,
          role: 'assistant',
          content: '',
          segments: [],
          wire: null,
          sharedHost: null,
          truncated: false,
          error: null,
          streaming: true,
        },
      ])
      setPhase('streaming')

      const closeReasoning = () => {
        if (thinkStart === null) return
        const ms = Date.now() - thinkStart
        thinkStart = null
        flushDeltas()
        setTurns((prev) => closeThink(prev, assistantId, ms))
      }

      const start = (streamGen: number, immediate: boolean) => {
        const requestId = crypto.randomUUID()
        requestIdRef.current = requestId
        if (immediate) {
          closeReasoning()
          setTurns((prev) => cancelRunningTools(prev, assistantId))
        }
        const wire = immediate
          ? [...convo, { role: 'system' as const, content: ANSWER_NOW_DIRECTIVE }]
          : convo

        let settled = false
        let toolCalls: AiToolCall[] | null = null
        let handedOff = false
        const settle = (error: string | null) => {
          if (settled || genRef.current !== streamGen) return
          settled = true
          if (requestIdRef.current === requestId) requestIdRef.current = null
          restartRef.current = null
          flushDeltas()
          closeReasoning()
          setTurns((prev) => settleTurn(prev, assistantId, error, truncated))
          setPhase('idle')
        }

        const channel = new Channel<StreamEvent>()
        channel.onmessage = (event) => {
          if (genRef.current !== streamGen) return
          if (event.type === 'delta') {
            closeReasoning()
            roundText += event.text
            queueDelta(assistantId, event.text, '')
          } else if (event.type === 'reasoning') {
            if (thinkStart === null) thinkStart = Date.now()
            queueDelta(assistantId, '', event.text)
          } else if (event.type === 'tool_calls') {
            closeReasoning()
            toolCalls = event.calls
          } else if (event.type === 'truncated') truncated = true
          else if (event.type === 'done') {
            // The budget is a hard stop.
            if (toolCalls !== null && !immediate && !toolsExhausted && !settled) {
              const calls = toolCalls
              toolCalls = null
              handedOff = true
              if (requestIdRef.current === requestId) requestIdRef.current = null
              void executeRound(streamGen, calls)
            } else settle(null)
          } else settle(event.message)
        }

        void commands
          .aiChat(
            {
              workspace_id: requestWorkspaceId,
              request_id: requestId,
              messages: wire,
              skip_reasoning: immediate,
              no_tools: toolsExhausted,
            },
            channel,
          )
          .then(
            () => {
              if (!handedOff) settle(null)
            },
            (error: unknown) => {
              if (!handedOff) settle(errorMessage(error))
            },
          )
      }

      // One tool round: vet the calls, run them in order, feed the results
      // back, and stream again.
      const executeRound = async (streamGen: number, rawCalls: AiToolCall[]) => {
        rounds += 1
        if (roundText !== '') queueDelta(assistantId, '\n\n', '')
        const planned = planToolCalls(rawCalls, allowedUrls, seenQueries)
        for (const p of planned) {
          if (p.action.kind === 'search') seenQueries.add(p.action.query.toLowerCase())
        }
        const round = roundSegments(planned)
        flushDeltas()
        setTurns((prev) => appendToolSegments(prev, assistantId, round.segments))

        const toolMessages: WireMessage[] = []
        for (let i = 0; i < planned.length; i += 1) {
          if (genRef.current !== streamGen) return
          const p = planned[i]
          const slot = round.slots[i]
          let ok = false
          let display = ''
          let content = ''
          let resultCount: number | undefined
          if (p.action.kind === 'reject') {
            content = p.action.reason
            display = p.action.reason
          } else if (p.action.kind === 'search') {
            try {
              const results = await commands.aiWebSearch(p.action.query)
              for (const r of results) allowedUrls.add(r.url)
              ok = true
              resultCount = results.length
              content = searchToolContent(results)
              display = content
            } catch (error: unknown) {
              display = errorMessage(error)
              content = `search failed: ${display}`
            }
          } else {
            try {
              const fetched = await commands.aiWebFetch(p.action.url)
              ok = true
              content = fetchToolContent(fetched)
              display = content
            } catch (error: unknown) {
              display = errorMessage(error)
              content = `fetch failed: ${display}`
            }
          }
          if (genRef.current !== streamGen) return
          setTurns((prev) =>
            settleToolCall(
              prev,
              assistantId,
              round.segments.length - 1 - slot.seg,
              slot.call,
              ok,
              previewToolResponse(display),
              resultCount,
            ),
          )
          toolMessages.push({ role: 'tool', content, tool_call_id: p.callId })
        }

        convo = [
          ...convo,
          { role: 'assistant', content: roundText, tool_calls: rawCalls },
          ...toolMessages,
        ]
        roundText = ''
        if (rounds >= MAX_TOOL_ROUNDS && !toolsExhausted) {
          toolsExhausted = true
          convo = [...convo, { role: 'system', content: TOOL_BUDGET_DIRECTIVE }]
        }
        start(streamGen, false)
      }

      restartRef.current = start
      start(gen, false)

      return true
    },
    [workspaceId, webEnabled, readPage, flushDeltas, queueDelta],
  )

  return { turns, phase, notice, send, answerNow, stop, reset }
}
