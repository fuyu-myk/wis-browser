import { memo, useEffect, useRef, type PointerEvent as ReactPointerEvent } from 'react'

import { AiMarkdown } from '@/components/AiMarkdown'
import type { CanvasCard } from '@/lib/canvas'
import { cx } from '@/lib/cx'
import { isWebUrl } from '@/lib/url'

import { HtmlFrame } from './CanvasHtml'
import { transformOf } from './dom'

export type Side = 'top' | 'right' | 'bottom' | 'left'

const SIDES: readonly Side[] = ['top', 'right', 'bottom', 'left']

export interface CardHandlers {
  register: (id: string, el: HTMLElement | null) => void
  link: (card: CanvasCard, side: Side, e: ReactPointerEvent<HTMLElement>) => void
  down: (card: CanvasCard, e: ReactPointerEvent<HTMLElement>) => void
  edit: (id: string) => void
  use: (id: string) => void
  resize: (card: CanvasCard, e: ReactPointerEvent<HTMLElement>) => void
  commit: (id: string, text: string) => void
}

interface CardProps {
  card: CanvasCard
  selected: boolean
  editing: boolean
  live: boolean
  handlers: CardHandlers
}

function openLink(href: string) {
  if (isWebUrl(href)) window.open(href, '_blank', 'noopener,noreferrer')
}

export const Card = memo(function Card({ card, selected, editing, live, handlers }: CardProps) {
  const html = card.kind === 'html'
  const still = !editing && !live
  return (
    <div
      className={cx('canvas-card', {
        'is-html': html,
        'is-selected': selected || live,
        'is-editing': editing,
        'is-live': live,
      })}
      data-card={card.id}
      ref={(el) => handlers.register(card.id, el)}
      style={{ width: card.w, height: card.h, transform: transformOf(card.x, card.y) }}
      onPointerDown={still ? (e) => handlers.down(card, e) : undefined}
      onDoubleClick={(e) => {
        e.stopPropagation()
        if (!still) return
        if (html) handlers.use(card.id)
        else handlers.edit(card.id)
      }}
    >
      {editing ? (
        <CardEditor text={card.text} code={html} onDone={(text) => handlers.commit(card.id, text)} />
      ) : html ? (
        <HtmlFrame html={card.text} live={live} />
      ) : (
        <AiMarkdown text={card.text} onOpenLink={openLink} />
      )}
      {html && selected && still && (
        <button
          type="button"
          className="canvas-html-edit"
          title="Edit HTML"
          onPointerDown={(e) => e.stopPropagation()}
          onDoubleClick={(e) => e.stopPropagation()}
          onClick={() => handlers.edit(card.id)}
        >
          {'</>'}
        </button>
      )}
      {live && <span className="canvas-html-hint">Esc to leave</span>}
      {html && !editing && (
        <span
          className="canvas-card-resize"
          onPointerDown={(e) => handlers.resize(card, e)}
          onDoubleClick={(e) => e.stopPropagation()}
        />
      )}
      {still &&
        SIDES.map((side) => (
          <span
            key={side}
            className="canvas-handle"
            data-side={side}
            onPointerDown={(e) => handlers.link(card, side, e)}
          />
        ))}
    </div>
  )
})

interface CardEditorProps {
  text: string
  code?: boolean
  onDone: (text: string) => void
}

export function CardEditor({ text, code = false, onDone }: CardEditorProps) {
  const ref = useRef<HTMLTextAreaElement>(null)
  const fit = () => {
    const el = ref.current
    if (!el || code) return
    el.style.height = 'auto'
    el.style.height = `${el.scrollHeight}px`
  }
  useEffect(() => {
    const el = ref.current
    if (!el) return
    fit()
    el.focus()
    if (code) el.select()
    else el.setSelectionRange(el.value.length, el.value.length)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  return (
    <textarea
      ref={ref}
      className={cx('canvas-card-editor', { 'is-code': code })}
      defaultValue={text}
      aria-label={code ? 'Card HTML' : 'Card text'}
      placeholder={code ? 'Paste or write HTML' : 'Write in Markdown'}
      spellCheck={!code}
      onInput={fit}
      onBlur={(e) => onDone(e.currentTarget.value)}
      onPointerDown={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        if (e.key === 'Escape') e.currentTarget.blur()
      }}
    />
  )
}
