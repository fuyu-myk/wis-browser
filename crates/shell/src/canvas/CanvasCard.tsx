import { memo, useEffect, useRef, type PointerEvent as ReactPointerEvent } from 'react'

import { AiMarkdown } from '@/components/AiMarkdown'
import type { CanvasCard } from '@/lib/canvas'
import { cx } from '@/lib/cx'
import { isWebUrl } from '@/lib/url'

import { transformOf } from './dom'

export type Side = 'top' | 'right' | 'bottom' | 'left'

const SIDES: readonly Side[] = ['top', 'right', 'bottom', 'left']

export interface CardHandlers {
  register: (id: string, el: HTMLElement | null) => void
  link: (card: CanvasCard, side: Side, e: ReactPointerEvent<HTMLElement>) => void
  down: (card: CanvasCard, e: ReactPointerEvent<HTMLElement>) => void
  edit: (id: string) => void
  commit: (id: string, text: string) => void
}

interface CardProps {
  card: CanvasCard
  selected: boolean
  editing: boolean
  handlers: CardHandlers
}

function openLink(href: string) {
  if (isWebUrl(href)) window.open(href, '_blank', 'noopener,noreferrer')
}

export const Card = memo(function Card({ card, selected, editing, handlers }: CardProps) {
  return (
    <div
      className={cx('canvas-card', { 'is-selected': selected, 'is-editing': editing })}
      data-card={card.id}
      ref={(el) => handlers.register(card.id, el)}
      style={{ width: card.w, transform: transformOf(card.x, card.y) }}
      onPointerDown={editing ? undefined : (e) => handlers.down(card, e)}
      onDoubleClick={(e) => {
        e.stopPropagation()
        if (!editing) handlers.edit(card.id)
      }}
    >
      {editing ? (
        <CardEditor text={card.text} onDone={(text) => handlers.commit(card.id, text)} />
      ) : (
        <AiMarkdown text={card.text} onOpenLink={openLink} />
      )}
      {!editing &&
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

export function CardEditor({ text, onDone }: { text: string; onDone: (text: string) => void }) {
  const ref = useRef<HTMLTextAreaElement>(null)
  const fit = () => {
    const el = ref.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${el.scrollHeight}px`
  }
  useEffect(() => {
    const el = ref.current
    if (!el) return
    fit()
    el.focus()
    el.setSelectionRange(el.value.length, el.value.length)
  }, [])
  return (
    <textarea
      ref={ref}
      className="canvas-card-editor"
      defaultValue={text}
      aria-label="Card text"
      placeholder="Write in Markdown"
      onInput={fit}
      onBlur={(e) => onDone(e.currentTarget.value)}
      onPointerDown={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        if (e.key === 'Escape') e.currentTarget.blur()
      }}
    />
  )
}
