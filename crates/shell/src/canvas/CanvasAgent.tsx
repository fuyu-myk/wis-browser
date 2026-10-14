import { useEffect, useLayoutEffect, useRef, useState, type FormEvent, type KeyboardEvent } from 'react'

import { AiMarkdown } from '@/components/AiMarkdown'
import { plainLine } from '@/lib/canvasAgent'
import { cx } from '@/lib/cx'
import { isWebUrl } from '@/lib/url'

import { useCanvasAgent, type AgentOptions, type CanvasAgent, type LogEntry } from './useCanvasAgent'

const MAX_PROMPT_PX = 160

function openLink(href: string) {
  if (isWebUrl(href)) window.open(href, '_blank', 'noopener,noreferrer')
}

export function CanvasAgentHost(options: AgentOptions) {
  const agent = useCanvasAgent(options)
  return (
    <>
      <AgentPill agent={agent} />
      <AgentPrompt agent={agent} />
    </>
  )
}

function pillLabel(agent: CanvasAgent): string {
  if (agent.activity === 'thinking') return 'Thinking…'
  if (agent.activity === 'working') {
    const step = [...agent.log].reverse().find((e) => e.kind === 'step')
    return step ? `${step.text}…` : 'Working…'
  }
  const last = agent.log[agent.log.length - 1]
  if (!last) return ''
  if (last.kind === 'error') return 'Could not finish'
  if (last.kind === 'reply') return plainLine(last.text)
  return last.text
}

function AgentPill({ agent }: { agent: CanvasAgent }) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const busy = agent.activity !== 'idle'

  useEffect(() => {
    if (!open) return
    const onDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false)
    }
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    window.addEventListener('pointerdown', onDown, true)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('pointerdown', onDown, true)
      window.removeEventListener('keydown', onKey)
    }
  }, [open])

  if (agent.log.length === 0 && !busy) return null
  return (
    <div className="canvas-agent" ref={rootRef}>
      <button
        type="button"
        className={cx('canvas-agent-pill', { 'is-busy': busy })}
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="canvas-agent-dot" aria-hidden />
        <span className="canvas-agent-label">{pillLabel(agent)}</span>
        {busy && (
          <span
            role="button"
            tabIndex={0}
            className="canvas-agent-stop"
            onClick={(e) => {
              e.stopPropagation()
              agent.stop()
            }}
          >
            Stop
          </span>
        )}
      </button>
      {open && <AgentLog agent={agent} />}
    </div>
  )
}

function AgentLog({ agent }: { agent: CanvasAgent }) {
  const end = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    end.current?.scrollIntoView({ block: 'end' })
  }, [agent.log])
  return (
    <div className="canvas-agent-log" role="log" aria-live="polite">
      {agent.log.map((entry) => (
        <LogRow key={entry.id} entry={entry} />
      ))}
      <div ref={end} className="canvas-agent-foot">
        <button type="button" disabled={agent.activity !== 'idle'} onClick={agent.clear}>
          Clear
        </button>
      </div>
    </div>
  )
}

function LogRow({ entry }: { entry: LogEntry }) {
  if (entry.kind === 'reply')
    return (
      <div className="canvas-agent-reply">
        <AiMarkdown text={entry.text} onOpenLink={openLink} />
      </div>
    )
  return <p className={`canvas-agent-${entry.kind}`}>{entry.text}</p>
}

function AgentPrompt({ agent }: { agent: CanvasAgent }) {
  const [text, setText] = useState('')
  const [multiline, setMultiline] = useState(false)
  const field = useRef<HTMLTextAreaElement>(null)
  const busy = agent.activity !== 'idle'
  const available = agent.status?.available ?? false

  useLayoutEffect(() => {
    const el = field.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, MAX_PROMPT_PX)}px`
    const style = getComputedStyle(el)
    const padding = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom)
    setMultiline(el.scrollHeight - padding > parseFloat(style.lineHeight) * 1.5)
  }, [text])

  const submit = (e?: FormEvent) => {
    e?.preventDefault()
    if (busy) {
      if (e) agent.stop()
      return
    }
    if (text.trim() === '' || !available) return
    agent.send(text)
    setText('')
  }
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault()
      submit()
    } else if (e.key === 'Escape') {
      e.currentTarget.blur()
    }
  }

  return (
    <form className={cx('canvas-prompt', { 'is-off': !available, 'is-multiline': multiline })} onSubmit={submit}>
      <textarea
        ref={field}
        rows={1}
        value={text}
        disabled={!available}
        aria-label="Ask the canvas agent"
        placeholder={
          agent.status === null || available
            ? 'Ask the canvas agent'
            : 'Set up AI in Settings › AI to use the canvas agent'
        }
        onChange={(e) => setText(e.currentTarget.value)}
        onFocus={agent.refresh}
        onKeyDown={onKeyDown}
      />
      {available && agent.status?.model && <span className="canvas-prompt-model">{agent.status.model}</span>}
      <button
        type="submit"
        className={cx('canvas-prompt-send', { 'is-stop': busy })}
        disabled={!busy && (!available || text.trim() === '')}
        aria-label={busy ? 'Stop' : 'Send'}
        title={busy ? 'Stop' : 'Send'}
      >
        <svg viewBox="0 0 24 24" aria-hidden>
          {busy ? <rect x="7" y="7" width="10" height="10" rx="2" /> : <path d="M12 19V5M5 12l7-7 7 7" />}
        </svg>
      </button>
    </form>
  )
}
