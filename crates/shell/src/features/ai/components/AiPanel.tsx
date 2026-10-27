import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { AiAskRequest } from '@/features/ai/lib/aiAsk'

import { commands } from '@/bindings/commands'
import type { WorkspaceId } from '@/bindings/types'
import {
  AI_PANEL_WIDTH_DEFAULT,
  clampPanelWidth,
  hostOf,
  SUMMARIZE_PROMPT,
  thinkDurationLabel,
  type AiTurn,
  type ThinkSegment,
  type ToolSegment,
  selectionContent,
} from '@/features/ai/lib/ai'
import { cx } from '@/lib/cx'
import {
  ChevronDownIcon,
  FileIcon,
  PlusIcon,
  SearchIcon,
  SendIcon,
  StopIcon,
  XIcon,
} from '@/lib/icons'
import { RESEARCH_PAGES } from '@/features/ai/lib/research'
import { showToast } from '@/lib/toast'
import { useAiChat, type AiPageSource } from '@/features/ai/hooks/useAiChat'
import { useDockWidth } from '@/hooks/useDockWidth'

import { AiMarkdown } from '@/features/ai/components/AiMarkdown'
import { AiModelPicker } from '@/features/ai/components/AiModelPicker'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { useAnnounce } from '@/hooks/useAnnounce'

const WIDTH_STORAGE_KEY = 'wis-ai-panel-width'

/** Distance from the bottom within which the transcript keeps auto-scrolling. */
const STICK_THRESHOLD_PX = 40

/** Lines the composer grows to before it starts scrolling instead. */
const COMPOSER_MAX_LINES = 3

/** How long a handed-off question waits for the panel to become able to send. */
const ASK_WAIT_MS = 5000

/** A queued prompt waiting for the assistant panel to be ready. */
interface PendingAsk {
  text: string
  /** When waiting stops being plausible and the fallback takes over. */
  until: number
  late: boolean
}

interface AiPanelProps {
  open: boolean
  /** A question sent from elsewhere, e.g. the page menu's "ask" row. */
  ask?: AiAskRequest | null
  /** Docked opposite the sidebar so the two chrome columns never crowd. */
  side: 'left' | 'right'
  workspaceId: WorkspaceId | null
  /** The active tab when its text can be read, null for chrome and error pages. */
  page: AiPageSource | null
  /** The configured model id; empty means the assistant cannot send yet. */
  model: string
  onChooseModel: (model: string) => Promise<void>
  /** Whether web tools can be offered for the current settings and platform. */
  webAccess: boolean
  /** Whether the tab, history and bookmark tools can be offered. */
  localData: boolean
  /** Whether the tab actions can be offered. */
  tabActions: boolean
  onClose: () => void
  onOpenSettings: () => void
}

/**
 * The docked assistant. It reserves chrome width rather than floating;
 * shrinking the content host is what makes the page reflow beside it.
 *
 * Page text only leaves the device when the user attaches it to a send, or
 * when local-data tools are on and the model reads a tab.
 */
export function AiPanel({
  open,
  ask,
  side,
  workspaceId,
  page,
  model,
  onChooseModel,
  webAccess,
  localData,
  tabActions,
  onClose,
  onOpenSettings,
}: AiPanelProps) {
  const chat = useAiChat(workspaceId, webAccess, localData, tabActions)
  useAnnounce(chat.phase === 'reading' && 'Reading the page')
  const [draft, setDraft] = useState('')
  const [attachPage, setAttachPage] = useState(false)
  const [research, setResearch] = useState(false)
  const { width, startResize } = useDockWidth(
    WIDTH_STORAGE_KEY,
    AI_PANEL_WIDTH_DEFAULT,
    clampPanelWidth,
    side,
  )
  /** A link whose text claims a different destination, held for confirmation. */
  const [linkPrompt, setLinkPrompt] = useState<{ href: string; text: string } | null>(null)

  const scrollRef = useRef<HTMLDivElement | null>(null)
  const composerRef = useRef<HTMLTextAreaElement | null>(null)
  const stickRef = useRef(true)

  const modelReady = model.trim() !== ''
  const busy = chat.phase !== 'idle'
  const canSend = draft.trim() !== '' && !busy && modelReady && workspaceId !== null

  // Consent covers the document in view: switching tab or navigating away has
  // to re-earn it rather than silently carry over to a different page.
  useEffect(() => {
    setAttachPage(false)
  }, [page?.id, page?.url])

  useEffect(() => {
    if (!webAccess) setResearch(false)
  }, [webAccess])

  useEffect(() => {
    if (open) composerRef.current?.focus()
  }, [open])

  useEffect(() => {
    const el = composerRef.current
    if (!el) return
    const styles = window.getComputedStyle(el)
    const line = Number.parseFloat(styles.lineHeight)
    if (!Number.isFinite(line)) return
    const padding =
      Number.parseFloat(styles.paddingTop) + Number.parseFloat(styles.paddingBottom)
    const border =
      Number.parseFloat(styles.borderTopWidth) + Number.parseFloat(styles.borderBottomWidth)

    const contentMax = line * COMPOSER_MAX_LINES + padding
    el.style.height = 'auto'
    const overflows = el.scrollHeight > contentMax
    el.style.height = `${(overflows ? contentMax : el.scrollHeight) + border}px`
    el.style.overflowY = overflows ? 'auto' : 'hidden'
  }, [draft, open])

  useEffect(() => {
    const el = scrollRef.current
    if (!el || !stickRef.current) return
    el.scrollTop = el.scrollHeight
  }, [chat.turns])

  const onScroll = useCallback(() => {
    const el = scrollRef.current
    if (!el) return
    stickRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < STICK_THRESHOLD_PX
  }, [])

  const submit = useCallback(
    (question: string, withPage: boolean, withResearch = false): Promise<boolean> => {
      if (busy || !modelReady || workspaceId === null) return Promise.resolve(false)
      setAttachPage(false)
      setResearch(false)
      return chat.send(question, withPage ? page : null, { research: withResearch && webAccess })
    },
    [busy, modelReady, workspaceId, chat, page, webAccess],
  )

  // A question from elsewhere. Keyed on the request's sequence.
  const askedRef = useRef(0)
  const draftRef = useRef(draft)
  draftRef.current = draft
  const [pendingAsk, setPendingAsk] = useState<PendingAsk | null>(null)
  useEffect(() => {
    if (!ask || ask.seq <= askedRef.current) return
    askedRef.current = ask.seq
    void commands.focusSelf().catch(() => undefined)
    composerRef.current?.focus()
    setPendingAsk({ text: ask.question, until: Date.now() + ASK_WAIT_MS, late: false })
  }, [ask])

  useEffect(() => {
    if (!pendingAsk) return

    if (modelReady && workspaceId !== null && !busy) {
      setPendingAsk(null)
      void submit(selectionContent(pendingAsk.text), false)
      return
    }

    const left = pendingAsk.until - Date.now()
    if (pendingAsk.late || left <= 0) {
      setPendingAsk(null)
      if (draftRef.current.trim() === '') setDraft(pendingAsk.text)
      else if (busy) showToast('The assistant is still answering, so your draft was kept.')
      else showToast('The assistant cannot answer that yet, so your draft was kept.')
      return
    }

    const timer = window.setTimeout(
      () => setPendingAsk((current) => (current ? { ...current, late: true } : null)),
      left,
    )
    return () => window.clearTimeout(timer)
  }, [pendingAsk, modelReady, workspaceId, busy, submit])

  // The composer is cleared only once the exchange is in the transcript: a
  // page read can still fail, and the typed question must survive that.
  const sendDraft = useCallback(() => {
    if (!canSend) return
    const question = draft
    void submit(question, attachPage, research).then((started) => {
      if (started) setDraft((current) => (current === question ? '' : current))
    })
  }, [canSend, submit, draft, attachPage, research])

  // A model link opens as a new tab, never a navigation: the panel is chrome,
  // and its window navigating away would replace the browser shell itself.
  const createLinkTab = useCallback(
    (href: string) => {
      if (workspaceId === null) return
      void commands.createTab(workspaceId, href).catch(() => undefined)
    },
    [workspaceId],
  )

  const openLink = useCallback(
    (href: string, text: string) => {
      void commands
        .aiAssessLink(href, text)
        .then(
          (assessment) => assessment.warnings.some((w) => w.code === 'deceptive_text'),
          () => false,
        )
        .then((deceptive) => {
          if (deceptive) setLinkPrompt({ href, text })
          else createLinkTab(href)
        })
    },
    [createLinkTab],
  )

  if (!open) return null

  return (
    <aside
      className={cx('kb-ai-panel', `is-${side}`)}
      style={{ width: `${width}px` }}
      aria-label="Assistant"
      onKeyDown={(e) => {
        if (e.key !== 'Escape') return
        e.preventDefault()
        onClose()
      }}
    >
      <div
        className="kb-ai-grip"
        onPointerDown={startResize}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize assistant"
      />
      <div className="kb-ai-head">
        <span className="kb-ai-title">Assistant</span>
        <AiModelPicker model={model} workspaceId={workspaceId} onChoose={onChooseModel} onError={showToast} />
        <div className="kb-ai-head-actions">
          {chat.turns.length > 0 && (
            <button
              type="button"
              className="kb-ai-btn kb-ai-icon"
              onClick={chat.reset}
              aria-label="New conversation"
              title="New conversation"
            >
              <PlusIcon />
            </button>
          )}
          <button
            type="button"
            className="kb-ai-btn kb-ai-icon"
            onClick={onClose}
            aria-label="Close assistant"
            title="Close assistant"
          >
            <XIcon />
          </button>
        </div>
      </div>

      <div className="kb-ai-scroll" ref={scrollRef} onScroll={onScroll}>
        {!modelReady ? (
          <div className="kb-ai-empty">
            <p>No model is set, so the assistant cannot send anything yet.</p>
            <button type="button" className="kb-ai-btn" onClick={onOpenSettings}>
              Open AI settings
            </button>
          </div>
        ) : chat.turns.length === 0 ? (
          <div className="kb-ai-empty">
            <p>Ask anything. Nothing is sent until you send it, and nothing is saved.</p>
            <button
              type="button"
              className="kb-ai-btn"
              onClick={() => void submit(SUMMARIZE_PROMPT, true)}
              disabled={page === null || busy}
              title={page === null ? 'Open a web page to summarise it' : undefined}
            >
              Summarise this page
            </button>
          </div>
        ) : (
          <ul className="kb-ai-list">
            {chat.turns.map((turn) => (
              <TurnRow
                key={turn.id}
                turn={turn}
                onOpenLink={openLink}
                onAnswerNow={chat.answerNow}
              />
            ))}
          </ul>
        )}
      </div>

      <div className="kb-ai-composer">
        {chat.approval !== null && (
          <div className="kb-ai-approval" role="group" aria-label={chat.approval.prompt}>
            <p className="kb-ai-approval-prompt" aria-live="polite">
              {chat.approval.prompt}
            </p>
            {chat.approval.items.length > 0 && (
              <ul className="kb-ai-approval-items">
                {chat.approval.items.map((item, i) => (
                  <li key={i}>{item}</li>
                ))}
              </ul>
            )}
            {chat.approval.note !== '' && <p>{chat.approval.note}</p>}
            <div className="kb-ai-approval-actions">
              <button type="button" className="kb-ai-btn" onClick={() => chat.answerApproval('cancel')}>
                {chat.approval.cancelLabel}
              </button>
              {chat.approval.retry && (
                <button type="button" className="kb-ai-btn" onClick={() => chat.answerApproval('retry')}>
                  Retry
                </button>
              )}
              <button
                type="button"
                className="kb-ai-btn is-primary"
                onClick={() => chat.answerApproval('accept')}
              >
                {chat.approval.confirmLabel}
              </button>
            </div>
          </div>
        )}
        {chat.notice !== null && <div className="kb-ai-notice">{chat.notice}</div>}
        {(page !== null || webAccess) && (
          <div className="kb-ai-toggles">
          {page !== null && (
            <button
              type="button"
              className={cx('kb-ai-attach', { on: attachPage })}
              onClick={() => {
                setAttachPage((v) => !v)
                setResearch(false)
              }}
              aria-pressed={attachPage}
              disabled={!modelReady}
            >
              <FileIcon />
              <span>
                {attachPage
                  ? "This page's text goes to your provider"
                  : "Include this page's text"}
              </span>
            </button>
          )}
          {webAccess && (
            <button
              type="button"
              className={cx('kb-ai-attach', { on: research })}
              onClick={() => {
                setResearch((v) => !v)
                setAttachPage(false)
              }}
              aria-pressed={research}
              disabled={!modelReady}
            >
              <SearchIcon />
              <span>
                {research
                  ? `Research: searches and reads up to ${RESEARCH_PAGES} sources`
                  : 'Research this question'}
              </span>
            </button>
          )}
          </div>
        )}
        <div className="kb-ai-input-row">
          <textarea
            ref={composerRef}
            className="kb-ai-input"
            rows={1}
            value={draft}
            placeholder={modelReady ? 'Ask the assistant' : 'Set a model to begin'}
            disabled={!modelReady}
            aria-label="Message the assistant"
            onChange={(e) => setDraft(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key !== 'Enter' || e.shiftKey) return
              e.preventDefault()
              sendDraft()
            }}
          />
          {busy ? (
            <button
              type="button"
              className="kb-ai-send"
              onClick={chat.stop}
              aria-label="Stop generating"
              title="Stop generating"
            >
              <StopIcon />
            </button>
          ) : (
            <button
              type="button"
              className="kb-ai-send is-primary"
              onClick={sendDraft}
              disabled={!canSend}
              aria-label="Send"
              title="Send"
            >
              <SendIcon />
            </button>
          )}
        </div>
        {chat.phase === 'reading' && (
          <div className="kb-ai-status">
            Reading the page
          </div>
        )}
      </div>
      <ConfirmDialog
        open={linkPrompt !== null}
        title="This link may be deceptive"
        message={
          linkPrompt !== null
            ? `The link reads "${linkPrompt.text}" but goes to ${hostOf(linkPrompt.href)}.`
            : undefined
        }
        confirmLabel="Open anyway"
        danger
        onConfirm={() => {
          if (linkPrompt !== null) createLinkTab(linkPrompt.href)
          setLinkPrompt(null)
        }}
        onCancel={() => setLinkPrompt(null)}
      />
    </aside>
  )
}

function TurnRow({
  turn,
  onOpenLink,
  onAnswerNow,
}: {
  turn: AiTurn
  onOpenLink: (href: string, text: string) => void
  onAnswerNow: () => void
}) {
  const [openSegments, setOpenSegments] = useState<ReadonlySet<number>>(new Set())
  const toggleSegment = (i: number) =>
    setOpenSegments((prev) => {
      const next = new Set(prev)
      if (next.has(i)) next.delete(i)
      else next.add(i)
      return next
    })

  // The newest live spell or running tool.
  let activeIndex = -1
  for (let i = turn.segments.length - 1; i >= 0; i -= 1) {
    const seg = turn.segments[i]
    if (seg.kind === 'think' ? seg.live : seg.status === 'running') {
      activeIndex = i
      break
    }
  }
  const answerable = turn.streaming && activeIndex !== -1 && !turn.researching
  const sources = turn.sources
  const citations = useMemo(
    () =>
      sources && sources.length > 0
        ? { count: sources.length, onCite: (n: number) => revealSource(turn.id, n) }
        : undefined,
    [sources, turn.id],
  )
  const waiting = turn.streaming && activeIndex === -1 && turn.content === ''

  return (
    <li className={cx('kb-ai-turn', `is-${turn.role}`)}>
      {turn.sharedHost !== null && (
        <span className="kb-ai-shared">Page content shared: {turn.sharedHost}</span>
      )}
      {turn.segments.map((seg, i) =>
        seg.kind === 'think' ? (
          <ThinkRow
            key={i}
            seg={seg}
            open={openSegments.has(i)}
            onToggle={() => toggleSegment(i)}
            answerNow={answerable && i === activeIndex ? onAnswerNow : null}
          />
        ) : (
          <ToolRow
            key={i}
            seg={seg}
            open={openSegments.has(i)}
            onToggle={() => toggleSegment(i)}
            answerNow={answerable && i === activeIndex ? onAnswerNow : null}
          />
        ),
      )}
      {waiting && (
        <div className="kb-ai-think">
          <div className="kb-ai-think-head">
            <span className="kb-ai-think-row is-static">
              <span className="kb-ai-think-label is-live">thinking...</span>
            </span>
            <AnswerNowButton onAnswerNow={onAnswerNow} />
          </div>
        </div>
      )}
      {turn.content !== '' &&
        (turn.role === 'assistant' ? (
          <AiMarkdown text={turn.content} onOpenLink={onOpenLink} citations={citations} />
        ) : (
          <p className="kb-ai-text">{turn.content}</p>
        ))}
      {sources && sources.length > 0 && (
        <ol className="kb-ai-sources" role="list" aria-label="Sources">
          {sources.map((source) => (
            <li key={source.n} id={sourceId(turn.id, source.n)}>
              <span className="kb-ai-source-n">{source.n}</span>
              <button
                type="button"
                className="kb-ai-source-link"
                title={source.url}
                onClick={() => onOpenLink(source.url, source.title)}
              >
                {source.title || hostOf(source.url)}
              </button>
              <span className="kb-ai-source-host">{hostOf(source.url)}</span>
            </li>
          ))}
        </ol>
      )}
      {turn.error !== null && (
        <p className="kb-ai-error" role="alert">
          {turn.error}
        </p>
      )}
      {turn.truncated && (
        <p className="kb-ai-truncated">Cut short at the provider's output limit.</p>
      )}
    </li>
  )
}

const sourceId = (turn: number, n: number) => `kb-ai-src-${turn}-${n}`

/** How long a cited source stays highlighted; matches `kb-ai-cited`. */
const CITED_MS = 1400

/** Bring a cited source into view and flash it. */
function revealSource(turn: number, n: number) {
  const row = document.getElementById(sourceId(turn, n))
  if (!row) return
  const still = window.matchMedia('(prefers-reduced-motion: reduce)').matches
  row.scrollIntoView({ behavior: still ? 'auto' : 'smooth', block: 'nearest' })
  row.classList.remove('is-cited')
  void row.offsetWidth
  row.classList.add('is-cited')
  window.setTimeout(() => row.classList.remove('is-cited'), CITED_MS)
}

function AnswerNowButton({ onAnswerNow }: { onAnswerNow: () => void }) {
  return (
    <button
      type="button"
      className="kb-ai-btn kb-ai-now"
      onClick={onAnswerNow}
      title="Cut this short and answer with what it currently has"
    >
      Answer now
    </button>
  )
}

function ThinkRow({
  seg,
  open,
  onToggle,
  answerNow,
}: {
  seg: ThinkSegment
  open: boolean
  onToggle: () => void
  answerNow: (() => void) | null
}) {
  const label = seg.live
    ? 'thinking...'
    : seg.ms !== null
      ? thinkDurationLabel(seg.ms)
      : 'thoughts'

  return (
    <div className="kb-ai-think">
      <div className="kb-ai-think-head">
        {seg.text !== '' ? (
          <button
            type="button"
            className="kb-ai-think-row"
            onClick={onToggle}
            aria-expanded={open}
          >
            <span className={cx('kb-ai-think-label', { 'is-live': seg.live })}>{label}</span>
            <ChevronDownIcon className={cx('kb-ai-think-chevron', { 'is-open': open })} />
          </button>
        ) : (
          <span className="kb-ai-think-row is-static">
            <span className={cx('kb-ai-think-label', { 'is-live': seg.live })}>{label}</span>
          </span>
        )}
        {answerNow !== null && <AnswerNowButton onAnswerNow={answerNow} />}
      </div>
      {open && seg.text !== '' && <p className="kb-ai-reasoning">{seg.text}</p>}
    </div>
  )
}

const TOOL_LABELS: Record<ToolSegment['tool'], string> = {
  plan: 'planning research',
  search: 'web search',
  fetch: 'web fetch',
  tabs: 'list tabs',
  read: 'read tab',
  history: 'history search',
  bookmarks: 'bookmark search',
  open: 'open tabs',
  switch: 'switch tab',
  close: 'close tabs',
  group: 'group tabs',
}

function ToolRow({
  seg,
  open,
  onToggle,
  answerNow,
}: {
  seg: ToolSegment
  open: boolean
  onToggle: () => void
  answerNow: (() => void) | null
}) {
  const running = seg.status === 'running'
  const label = seg.note ?? `using tool: ${TOOL_LABELS[seg.tool]}`

  return (
    <div className="kb-ai-think">
      <div className="kb-ai-think-head">
        <button
          type="button"
          className="kb-ai-think-row"
          onClick={onToggle}
          aria-expanded={open}
        >
          <span
            className={cx('kb-ai-think-label', {
              'is-live': running,
              'is-failed': seg.status === 'failed',
            })}
          >
            {label}
          </span>
          <ChevronDownIcon className={cx('kb-ai-think-chevron', { 'is-open': open })} />
        </button>
        {answerNow !== null && <AnswerNowButton onAnswerNow={answerNow} />}
      </div>
      {open && (
        <div className="kb-ai-tool-body">
          {seg.calls.map((call, i) => (
            <div key={i} className="kb-ai-tool-call">
              <span className="kb-ai-tool-detail">{call.detail}</span>
              {call.response !== '' && <pre className="kb-ai-tool-resp">{call.response}</pre>}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
