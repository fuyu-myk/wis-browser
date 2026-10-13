import {
  memo,
  useEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react'

import { AiMarkdown } from '@/components/AiMarkdown'
import {
  addCard,
  newCard,
  removeCard,
  toCanvas,
  updateCard,
  withView,
  zoomAt,
  type CanvasCard,
  type CanvasDoc,
  type CanvasView,
} from '@/lib/canvas'
import { cx } from '@/lib/cx'
import { isWebUrl } from '@/lib/url'

/** How long the view rests before it is saved. */
const VIEW_SETTLE_MS = 400
/** Pointer travel that turns a press on a card into a drag. */
const DRAG_SLOP = 3
const LINE_PX = 16

/** Safari's pinch event; not in the DOM typings. */
interface GestureEvent extends UIEvent {
  scale: number
  clientX: number
  clientY: number
}

interface CanvasSurfaceProps {
  doc: CanvasDoc
  onChange: (doc: CanvasDoc) => void
}

const transformOf = (x: number, y: number) => `translate3d(${x}px,${y}px,0)`
const viewTransform = ({ x, y, k }: CanvasView) => `translate3d(${x}px,${y}px,0) scale(${k})`

function openLink(href: string) {
  if (isWebUrl(href)) window.open(href, '_blank', 'noopener,noreferrer')
}

/**
 * The canvas: React renders cards when the document changes, while pan, zoom
 * and drags write transforms directly, once per frame.
 */
export function CanvasSurface({ doc, onChange }: CanvasSurfaceProps) {
  const viewportRef = useRef<HTMLDivElement>(null)
  const layerRef = useRef<HTMLDivElement>(null)
  const view = useRef<CanvasView>(doc.view)
  const docRef = useRef(doc)
  docRef.current = doc
  const changeRef = useRef(onChange)
  changeRef.current = onChange
  const cardEls = useRef(new Map<string, HTMLElement>())
  const dragged = useRef<{ id: string; x: number; y: number } | null>(null)
  const frame = useRef(0)
  const viewTimer = useRef(0)
  const [selected, setSelected] = useState<string | null>(null)
  const [editing, setEditing] = useState<string | null>(null)

  const paint = () => {
    frame.current = 0
    if (layerRef.current) layerRef.current.style.transform = viewTransform(view.current)
    const d = dragged.current
    if (d) {
      const el = cardEls.current.get(d.id)
      if (el) el.style.transform = transformOf(d.x, d.y)
    }
  }
  const schedule = () => {
    if (frame.current === 0) frame.current = requestAnimationFrame(paint)
  }
  const settleView = () => {
    window.clearTimeout(viewTimer.current)
    viewTimer.current = window.setTimeout(
      () => changeRef.current(withView(docRef.current, view.current)),
      VIEW_SETTLE_MS,
    )
  }
  const localPoint = (clientX: number, clientY: number) => {
    const rect = viewportRef.current?.getBoundingClientRect()
    return { x: clientX - (rect?.left ?? 0), y: clientY - (rect?.top ?? 0) }
  }

  useEffect(() => {
    const el = viewportRef.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && (e.target as Element).closest('textarea')) return
      e.preventDefault()
      const scale = e.deltaMode === 1 ? LINE_PX : 1
      const p = localPoint(e.clientX, e.clientY)
      view.current = e.ctrlKey
        ? zoomAt(view.current, p.x, p.y, Math.exp(-e.deltaY * scale * 0.01))
        : { ...view.current, x: view.current.x - e.deltaX * scale, y: view.current.y - e.deltaY * scale }
      schedule()
      settleView()
    }
    let pinch = 1
    const onGestureStart = (e: Event) => {
      e.preventDefault()
      pinch = 1
    }
    const onGestureChange = (e: Event) => {
      e.preventDefault()
      const g = e as GestureEvent
      const p = localPoint(g.clientX, g.clientY)
      view.current = zoomAt(view.current, p.x, p.y, g.scale / pinch)
      pinch = g.scale
      schedule()
      settleView()
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    el.addEventListener('gesturestart', onGestureStart)
    el.addEventListener('gesturechange', onGestureChange)
    return () => {
      el.removeEventListener('wheel', onWheel)
      el.removeEventListener('gesturestart', onGestureStart)
      el.removeEventListener('gesturechange', onGestureChange)
      window.clearTimeout(viewTimer.current)
      cancelAnimationFrame(frame.current)
    }
    // The handlers read refs only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (editing !== null || (e.target as Element).closest('input, textarea')) return
      if (e.key === 'Escape') setSelected(null)
      if ((e.key === 'Delete' || e.key === 'Backspace') && selected !== null) {
        e.preventDefault()
        changeRef.current(removeCard(docRef.current, selected))
        setSelected(null)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [editing, selected])

  const onBackgroundDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || (e.target as Element).closest('.canvas-card')) return
    setSelected(null)
    const start = { px: e.clientX, py: e.clientY, x: view.current.x, y: view.current.y }
    const move = (ev: PointerEvent) => {
      view.current = { ...view.current, x: start.x + ev.clientX - start.px, y: start.y + ev.clientY - start.py }
      schedule()
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      settleView()
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  const onBackgroundDoubleClick = (e: ReactMouseEvent<HTMLDivElement>) => {
    if ((e.target as Element).closest('.canvas-card')) return
    const p = localPoint(e.clientX, e.clientY)
    const at = toCanvas(view.current, p.x, p.y)
    const card = newCard(at.x, at.y)
    changeRef.current(addCard(docRef.current, card))
    setSelected(card.id)
    setEditing(card.id)
  }

  const handlers = useRef({
    register(id: string, el: HTMLElement | null) {
      if (el) cardEls.current.set(id, el)
      else cardEls.current.delete(id)
    },
    down(card: CanvasCard, e: ReactPointerEvent<HTMLElement>) {
      if (e.button !== 0) return
      e.stopPropagation()
      setSelected(card.id)
      const start = { px: e.clientX, py: e.clientY }
      let moving = false
      const move = (ev: PointerEvent) => {
        const dx = ev.clientX - start.px
        const dy = ev.clientY - start.py
        if (!moving && Math.hypot(dx, dy) < DRAG_SLOP) return
        moving = true
        const k = view.current.k
        dragged.current = { id: card.id, x: card.x + dx / k, y: card.y + dy / k }
        schedule()
      }
      const up = () => {
        window.removeEventListener('pointermove', move)
        window.removeEventListener('pointerup', up)
        const d = dragged.current
        dragged.current = null
        if (moving && d) changeRef.current(updateCard(docRef.current, d.id, { x: d.x, y: d.y }))
      }
      window.addEventListener('pointermove', move)
      window.addEventListener('pointerup', up)
    },
    edit(id: string) {
      setSelected(id)
      setEditing(id)
    },
    commit(id: string, text: string) {
      setEditing(null)
      const current = docRef.current.cards.find((c) => c.id === id)
      if (!current) return
      if (text.trim() === '') changeRef.current(removeCard(docRef.current, id))
      else if (text !== current.text) changeRef.current(updateCard(docRef.current, id, { text }))
    },
  }).current

  return (
    <div
      className="canvas-viewport"
      ref={viewportRef}
      onPointerDown={onBackgroundDown}
      onDoubleClick={onBackgroundDoubleClick}
    >
      <div className="canvas-layer" ref={layerRef} style={{ transform: viewTransform(view.current) }}>
        {doc.cards.map((card) => (
          <Card
            key={card.id}
            card={card}
            selected={selected === card.id}
            editing={editing === card.id}
            handlers={handlers}
          />
        ))}
      </div>
      {doc.cards.length === 0 && <p className="canvas-hint">Double-click anywhere to add a card</p>}
    </div>
  )
}

interface CardProps {
  card: CanvasCard
  selected: boolean
  editing: boolean
  handlers: {
    register: (id: string, el: HTMLElement | null) => void
    down: (card: CanvasCard, e: ReactPointerEvent<HTMLElement>) => void
    edit: (id: string) => void
    commit: (id: string, text: string) => void
  }
}

const Card = memo(function Card({ card, selected, editing, handlers }: CardProps) {
  return (
    <div
      className={cx('canvas-card', { 'is-selected': selected, 'is-editing': editing })}
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
      ) : card.text.trim() === '' ? (
        <p className="canvas-card-empty">Empty card</p>
      ) : (
        <AiMarkdown text={card.text} onOpenLink={openLink} />
      )}
    </div>
  )
})

function CardEditor({ text, onDone }: { text: string; onDone: (text: string) => void }) {
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
      onKeyDown={(e) => {
        if (e.key === 'Escape') e.currentTarget.blur()
      }}
    />
  )
}
