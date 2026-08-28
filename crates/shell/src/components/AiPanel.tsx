import { useCallback, useEffect, useRef, useState, type PointerEvent } from 'react'

import { commands } from '@/bindings/commands'
import type { WorkspaceId } from '@/bindings/types'
import {
  AI_PANEL_WIDTH_DEFAULT,
  clampPanelWidth,
  hostOf,
  SUMMARIZE_PROMPT,
  thinkDurationLabel,
  type AiTurn,
} from '@/lib/ai'
import { cx } from '@/lib/cx'
import { ChevronDownIcon, FileIcon, PlusIcon, SendIcon, StopIcon, XIcon } from '@/lib/icons'
import { useAiChat, type AiPageSource } from '@/hooks/useAiChat'

import { AiMarkdown } from '@/components/AiMarkdown'
import { ConfirmDialog } from '@/components/ConfirmDialog'

const WIDTH_STORAGE_KEY = 'wis-ai-panel-width'

/** Distance from the bottom within which the transcript keeps auto-scrolling. */
const STICK_THRESHOLD_PX = 40

/** Lines the composer grows to before it starts scrolling instead. */
const COMPOSER_MAX_LINES = 3

interface AiPanelProps {
  open: boolean
  /** Docked opposite the sidebar so the two chrome columns never crowd. */
  side: 'left' | 'right'
  workspaceId: WorkspaceId | null
  /** The active tab when its text can be read, null for chrome and error pages. */
  page: AiPageSource | null
  /** The configured model id; empty means the assistant cannot send yet. */
  model: string
  onClose: () => void
  onOpenSettings: () => void
}

function readStoredWidth(): number {
  const raw = window.localStorage.getItem(WIDTH_STORAGE_KEY)
  const parsed = raw === null ? Number.NaN : Number.parseInt(raw, 10)
  return clampPanelWidth(Number.isNaN(parsed) ? AI_PANEL_WIDTH_DEFAULT : parsed)
}

/**
 * The docked assistant. It reserves chrome width rather than floating;
 * shrinking the content host is what makes the page reflow beside it.
 *
 * Page text only ever leaves the device when the user attaches it to a send.
 */
export function AiPanel({
  open,
  side,
  workspaceId,
  page,
  model,
  onClose,
  onOpenSettings,
}: AiPanelProps) {
  const chat = useAiChat(workspaceId)
  const [draft, setDraft] = useState('')
  const [attachPage, setAttachPage] = useState(false)
  const [width, setWidth] = useState(readStoredWidth)
  /** A link whose text claims a different destination, held for confirmation. */
  const [linkPrompt, setLinkPrompt] = useState<{ href: string; text: string } | null>(null)

  const scrollRef = useRef<HTMLDivElement | null>(null)
  const composerRef = useRef<HTMLTextAreaElement | null>(null)
  const stickRef = useRef(true)
  const widthRef = useRef(width)
  widthRef.current = width

  const modelReady = model.trim() !== ''
  const busy = chat.phase !== 'idle'
  const canSend = draft.trim() !== '' && !busy && modelReady && workspaceId !== null

  // Consent covers the document in view: switching tab or navigating away has
  // to re-earn it rather than silently carry over to a different page.
  useEffect(() => {
    setAttachPage(false)
  }, [page?.id, page?.url])

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
    (question: string, withPage: boolean): Promise<boolean> => {
      if (busy || !modelReady || workspaceId === null) return Promise.resolve(false)
      setAttachPage(false)
      return chat.send(question, withPage ? page : null)
    },
    [busy, modelReady, workspaceId, chat, page],
  )

  // The composer is cleared only once the exchange is in the transcript: a
  // page read can still fail, and the typed question must survive that.
  const sendDraft = useCallback(() => {
    if (!canSend) return
    const question = draft
    void submit(question, attachPage).then((started) => {
      if (started) setDraft((current) => (current === question ? '' : current))
    })
  }, [canSend, submit, draft, attachPage])

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

  const startResize = useCallback(
    (e: PointerEvent<HTMLDivElement>) => {
      if (e.button !== 0) return
      e.preventDefault()
      const startX = e.clientX
      const startWidth = widthRef.current
      const setActive = (active: boolean) =>
        window.dispatchEvent(new CustomEvent('wis-pane-resize', { detail: active }))

      const onMove = (ev: globalThis.PointerEvent) => {
        const delta = side === 'right' ? startX - ev.clientX : ev.clientX - startX
        setWidth(clampPanelWidth(startWidth + delta))
      }
      const onUp = () => {
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
        setActive(false)
        window.localStorage.setItem(WIDTH_STORAGE_KEY, String(widthRef.current))
      }

      setActive(true)
      window.addEventListener('pointermove', onMove)
      window.addEventListener('pointerup', onUp)
    },
    [side],
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
        {modelReady && <span className="kb-ai-model">{model}</span>}
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
        {chat.notice !== null && <div className="kb-ai-notice">{chat.notice}</div>}
        {page !== null && (
          <button
            type="button"
            className={cx('kb-ai-attach', { on: attachPage })}
            onClick={() => setAttachPage((v) => !v)}
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
        {chat.phase === 'reading' && <div className="kb-ai-status">Reading the page</div>}
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
  const [expanded, setExpanded] = useState(false)
  const thinking = turn.streaming && turn.content === ''
  const hasReasoning = turn.reasoning !== ''

  return (
    <li className={cx('kb-ai-turn', `is-${turn.role}`)}>
      {turn.sharedHost !== null && (
        <span className="kb-ai-shared">Page content shared: {turn.sharedHost}</span>
      )}
      {(thinking || hasReasoning) && (
        <div className="kb-ai-think">
          <div className="kb-ai-think-head">
            {hasReasoning ? (
              <button
                type="button"
                className="kb-ai-think-row"
                onClick={() => setExpanded((v) => !v)}
                aria-expanded={expanded}
              >
                <span className={cx('kb-ai-think-label', { 'is-live': thinking })}>
                  {thinking
                    ? 'thinking...'
                    : turn.reasoningMs !== null
                      ? thinkDurationLabel(turn.reasoningMs)
                      : 'thoughts'}
                </span>
                <ChevronDownIcon className={cx('kb-ai-think-chevron', { 'is-open': expanded })} />
              </button>
            ) : (
              <span className="kb-ai-think-row is-static">
                <span className="kb-ai-think-label is-live">thinking...</span>
              </span>
            )}
            {thinking && (
              <button
                type="button"
                className="kb-ai-btn kb-ai-now"
                onClick={onAnswerNow}
                title="Cut thinking short and answer with what it currently has"
              >
                Answer now
              </button>
            )}
          </div>
          {expanded && <p className="kb-ai-reasoning">{turn.reasoning}</p>}
        </div>
      )}
      {turn.content !== '' &&
        (turn.role === 'assistant' ? (
          <AiMarkdown text={turn.content} onOpenLink={onOpenLink} />
        ) : (
          <p className="kb-ai-text">{turn.content}</p>
        ))}
      {turn.error !== null && <p className="kb-ai-error">{turn.error}</p>}
      {turn.truncated && (
        <p className="kb-ai-truncated">Cut short at the provider's output limit.</p>
      )}
    </li>
  )
}
