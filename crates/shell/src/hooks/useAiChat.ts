import { useCallback, useEffect, useRef, useState } from 'react'
import { Channel } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { AiTabRead, AiToolCall, StreamEvent, TabId, WorkspaceId } from '@/bindings/types'
import {
  ANSWER_NOW_DIRECTIVE,
  appendDelta,
  appendToolSegments,
  bookmarksToolContent,
  cancelRunningTools,
  closeThink,
  extractUrls,
  fetchToolContent,
  finalizeInterrupted,
  historyToolContent,
  hostOf,
  MAX_TOOL_ROUNDS,
  planToolCalls,
  actionApproval,
  actionDone,
  declinedOutcome,
  previewToolResponse,
  readsLocalData,
  searchApproval,
  tabName,
  roundSegments,
  SEARCH_DECLINED,
  searchToolContent,
  settleToolCall,
  settleTurn,
  tabsToolContent,
  tabToolContent,
  TOOL_BUDGET_DIRECTIVE,
  wireMessages,
  type AiTurn,
  type ApprovalAnswer,
  type ApprovalRequest,
  type PlannedAction,
  type TabAction,
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
  /** The tool call awaiting the user's answer, if any. */
  approval: ApprovalRequest | null
  answerApproval: (answer: ApprovalAnswer) => void
}

interface ToolOutcome {
  ok: boolean
  /** What the segment's dropdown shows. */
  display: string
  /** What the model reads back. */
  content: string
  resultCount?: number
  /** Header for the settled segment, overriding the usual one. */
  note?: string
}

/** One turn's tool state, shared by its rounds. */
interface ToolContext {
  /** URLs a fetch may load; search results add to it. */
  allowedUrls: Set<string>
  readTab: (source: AiPageSource) => Promise<SharedPage>
  /** Asks the user before a tab action, or a web search once the turn has read local data. */
  confirm: (request: ApprovalRequest) => Promise<ApprovalAnswer>
  /** Set once the turn has read tabs, history or bookmarks. */
  readLocal: boolean
}

/** The tool name a failure is reported under, as the model knows it. */
const FAILED_AS: Record<Exclude<PlannedAction['kind'], 'reject'>, string> = {
  search: 'search',
  fetch: 'fetch',
  tabs: 'list_tabs',
  read: 'read_tab',
  history: 'search_history',
  bookmarks: 'search_bookmarks',
  open: 'open_tabs',
  switch: 'switch_tab',
  close: 'close_tabs',
  group: 'group_tabs',
}

const TAB_ACTIONS: ReadonlySet<PlannedAction['kind']> = new Set(['open', 'switch', 'close', 'group'])

const isTabAction = (action: PlannedAction): action is TabAction => TAB_ACTIONS.has(action.kind)

const finished = (done: { note: string; content: string }, resultCount?: number): ToolOutcome => ({
  ok: true,
  display: done.content,
  content: done.content,
  note: done.note,
  resultCount,
})

const succeeded = (content: string, resultCount?: number): ToolOutcome => ({
  ok: true,
  display: content,
  content,
  resultCount,
})

async function executeAction(
  action: Exclude<PlannedAction, { kind: 'reject' }>,
  ctx: ToolContext,
): Promise<ToolOutcome> {
  switch (action.kind) {
    case 'search': {
      const results = await commands.aiWebSearch(action.query)
      for (const r of results) ctx.allowedUrls.add(r.url)
      return succeeded(searchToolContent(results), results.length)
    }
    case 'fetch':
      return succeeded(fetchToolContent(await commands.aiWebFetch(action.url)))
    case 'tabs': {
      const tabs = await commands.aiListTabs()
      return succeeded(tabsToolContent(tabs), tabs.length)
    }
    case 'read': {
      const target = await commands.aiResolveTab(action.tab)
      const page = await ctx.readTab(target)
      return succeeded(tabToolContent({ ...page, url: target.url }))
    }
    case 'history': {
      const hits = await commands.aiSearchHistory(action.query)
      return succeeded(historyToolContent(hits), hits.length)
    }
    case 'bookmarks': {
      const hits = await commands.aiSearchBookmarks(action.query)
      return succeeded(bookmarksToolContent(hits), hits.length)
    }
    case 'open': {
      const opened = await commands.aiOpenTabs(action.urls)
      return finished(actionDone('open', opened), opened.length)
    }
    case 'switch': {
      const tab = await commands.aiSwitchTab(action.tab)
      return finished(actionDone('switch', [tabName(tab)]))
    }
    case 'close': {
      const closed = await commands.aiCloseTabs(action.tabs)
      return finished(actionDone('close', closed.map(tabName)), closed.length)
    }
    case 'group': {
      const grouped = await commands.aiGroupTabs(action.tabs, action.folder)
      const done = actionDone('group', grouped.tabs.map(tabName), {
        name: grouped.folder,
        created: grouped.created,
      })
      return finished(done, grouped.tabs.length)
    }
  }
}

type Approved = { action: Exclude<PlannedAction, { kind: 'reject' }> } | { declined: ToolOutcome }

const CANCELLED = declinedOutcome('cancel')

/** A call the user turned down, or that a stop left behind, settled as a plain row. */
const declined = ({ note, content }: { note: string; content: string }): Approved => ({
  declined: { ok: true, display: content, content, note },
})

/** The action bound to what its approval card showed: full tab ids, parsed URLs. */
function boundTo(action: TabAction, tabs: readonly AiTabRead[], urls: readonly string[]): TabAction {
  const ids = tabs.map((t) => t.id)
  switch (action.kind) {
    case 'open':
      return { ...action, urls: [...urls] }
    case 'switch':
      return { ...action, tab: ids[0] ?? action.tab }
    case 'close':
    case 'group':
      return { ...action, tabs: ids }
  }
}

/**
 * Ask the user first where a call needs it: every tab action, and a web search
 * once the turn has read local data. An accepted tab action runs on exactly
 * what its card showed. `live` turns false once a stop, Answer now or
 * workspace change abandons the turn, so no card outlives it.
 */
async function askFirst(
  action: Exclude<PlannedAction, { kind: 'reject' }>,
  ctx: ToolContext,
  live: () => boolean,
): Promise<Approved> {
  if (action.kind === 'search') {
    if (!ctx.readLocal || (await ctx.confirm(searchApproval(action.query))) === 'accept') {
      return { action }
    }
    return declined({ note: 'search declined', content: SEARCH_DECLINED })
  }
  if (!isTabAction(action)) return { action }
  const named = action.kind === 'switch' ? [action.tab] : action.kind === 'open' ? [] : action.tabs
  const urls = action.kind === 'open' ? await commands.aiPreviewUrls(action.urls) : []
  const tabs = named.length > 0 ? await commands.aiPreviewTabs(named) : []
  if (!live()) return declined(CANCELLED)
  const answer = await ctx.confirm(actionApproval(action, { tabs, urls }, ctx.readLocal))
  if (answer !== 'accept') return declined(declinedOutcome(answer))
  return live() ? { action: boundTo(action, tabs, urls) } : declined(CANCELLED)
}

/**
 * Run one vetted tool call. Every native command re-checks its own setting,
 * so a call the model was never offered still fails safe. Once the turn has
 * read local data, a web search waits for the user, since its query is the
 * one place that data could leave for a third party.
 */
async function runToolAction(
  action: PlannedAction,
  ctx: ToolContext,
  live: () => boolean,
): Promise<ToolOutcome> {
  if (action.kind === 'reject') return { ok: false, display: action.reason, content: action.reason }
  try {
    const approved = await askFirst(action, ctx, live)
    if ('declined' in approved) return approved.declined
    const outcome = await executeAction(approved.action, ctx)
    if (readsLocalData(action.kind)) ctx.readLocal = true
    return outcome
  } catch (error: unknown) {
    const display = errorMessage(error)
    return { ok: false, display, content: `${FAILED_AS[action.kind]} failed: ${display}` }
  }
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
 * `webEnabled`, `localEnabled` and `actionsEnabled` mark which tools can
 * actually be offered for the current settings; they steer the system prompt
 * and nothing else.
 */
export function useAiChat(
  workspaceId: WorkspaceId | null,
  webEnabled: boolean,
  localEnabled: boolean,
  actionsEnabled: boolean,
): AiChat {
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

  const approvalRef = useRef<((answer: ApprovalAnswer) => void) | null>(null)
  const [approval, setApproval] = useState<ApprovalRequest | null>(null)

  const answerApproval = useCallback((answer: ApprovalAnswer) => {
    const resolve = approvalRef.current
    approvalRef.current = null
    setApproval(null)
    resolve?.(answer)
  }, [])

  const askApproval = useCallback(
    (request: ApprovalRequest) =>
      new Promise<ApprovalAnswer>((resolve) => {
        approvalRef.current?.('cancel')
        approvalRef.current = resolve
        setApproval(request)
      }),
    [],
  )

  /** Abandon whatever is in flight, upstream request included. */
  const abandonInFlight = useCallback(() => {
    genRef.current += 1
    answerApproval('cancel')
    abandonPageRead()
    restartRef.current = null
    const requestId = requestIdRef.current
    if (requestId !== null) {
      requestIdRef.current = null
      void commands.aiCancel(requestId).catch(() => undefined)
    }
  }, [abandonPageRead, answerApproval])

  const answerNow = useCallback(() => {
    const restart = restartRef.current
    if (restart === null) return
    answerApproval('cancel')
    const requestId = requestIdRef.current
    if (requestId !== null) {
      requestIdRef.current = null
      void commands.aiCancel(requestId).catch(() => undefined)
    }
    flushDeltas()
    restart(++genRef.current, true)
  }, [flushDeltas, answerApproval])

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
      let convo: WireMessage[] = wireMessages(
        history,
        asked,
        shared,
        webEnabled,
        localEnabled,
        actionsEnabled,
      )
      // Fetches are allowed only for URLs the user wrote or a search returned.
      const allowedUrls = new Set<string>(extractUrls(asked))
      for (const t of history) {
        if (t.role !== 'user') continue
        for (const u of extractUrls(t.content)) allowedUrls.add(u)
      }
      const toolContext: ToolContext = {
        allowedUrls,
        readTab: readPage,
        confirm: askApproval,
        readLocal: false,
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
          const { ok, display, content, resultCount, note } = await runToolAction(
            p.action,
            toolContext,
            () => genRef.current === streamGen,
          )
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
              note,
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
    [
      workspaceId,
      webEnabled,
      localEnabled,
      actionsEnabled,
      readPage,
      askApproval,
      flushDeltas,
      queueDelta,
    ],
  )

  return { turns, phase, notice, send, answerNow, stop, reset, approval, answerApproval }
}
