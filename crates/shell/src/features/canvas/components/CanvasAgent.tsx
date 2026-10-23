import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'

import { AiMarkdown } from '@/features/ai/components/AiMarkdown'
import { thinkDurationLabel } from '@/features/ai/lib/ai'
import { plainLine } from '@/features/canvas/lib/canvasAgent'
import { cx } from '@/lib/cx'
import { ChevronDownIcon } from '@/lib/icons'
import { isWebUrl } from '@/lib/url'

import { CanvasPrompt } from '@/features/canvas/components/CanvasPrompt'
import { useCanvasAgent, type AgentOptions, type CanvasAgent, type LogEntry } from '@/features/canvas/hooks/useCanvasAgent'

function openLink(href: string) {
  if (isWebUrl(href)) window.open(href, '_blank', 'noopener,noreferrer')
}

export function CanvasAgentHost(options: AgentOptions) {
  const agent = useCanvasAgent(options)
  if (agent.status?.supported === false) return null
  return (
    <>
      <AgentPill agent={agent} />
      <CanvasPrompt agent={agent} />
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
        <span className={cx('canvas-agent-label', { 'is-live': busy })}>{pillLabel(agent)}</span>
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

const capitalised = (text: string) => text.charAt(0).toUpperCase() + text.slice(1)

function LogRow({ entry }: { entry: LogEntry }) {
  if (entry.kind === 'reply')
    return (
      <div className="canvas-agent-reply">
        <AiMarkdown text={entry.text} onOpenLink={openLink} />
      </div>
    )
  if (entry.kind === 'think') {
    const label = entry.live
      ? 'Thinking…'
      : entry.ms !== null
        ? capitalised(thinkDurationLabel(entry.ms))
        : 'Thoughts'
    return (
      <Disclosure label={label} live={entry.live}>
        {entry.text !== '' && <p className="canvas-agent-reasoning">{entry.text}</p>}
      </Disclosure>
    )
  }
  if (entry.kind === 'user')
    return (
      <div className="canvas-agent-user">
        {entry.text}
        {entry.files.length > 0 && (
          <span className="canvas-agent-files">{entry.files.join(' · ')}</span>
        )}
      </div>
    )
  if (entry.kind === 'step')
    return (
      <Disclosure label={entry.text} live={false}>
        {entry.details.length > 0 && (
          <ul className="canvas-agent-details">
            {entry.details.map((detail, i) => (
              <li key={i}>{detail}</li>
            ))}
          </ul>
        )}
      </Disclosure>
    )
  return <p className={`canvas-agent-${entry.kind}`}>{entry.text}</p>
}

/** A log line that opens with a caret when it has more to show. */
function Disclosure({ label, live, children }: { label: string; live: boolean; children: ReactNode }) {
  const [open, setOpen] = useState(false)
  const more = children !== false && children !== null && children !== undefined
  const text = <span className={cx('canvas-agent-line', { 'is-live': live })}>{label}</span>
  return (
    <div className="canvas-agent-disclosure">
      {more ? (
        <button type="button" className="canvas-agent-toggle" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
          {text}
          <ChevronDownIcon className={cx('canvas-agent-chevron', { 'is-open': open })} />
        </button>
      ) : (
        <span className="canvas-agent-toggle is-static">{text}</span>
      )}
      {open && more && children}
    </div>
  )
}
