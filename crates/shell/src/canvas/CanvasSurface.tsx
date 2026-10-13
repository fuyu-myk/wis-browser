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
  boxBetween,
  cardsTouching,
  moveCards,
  newCard,
  removeCard,
  removeCards,
  toCanvas,
  updateCard,
  zoomAt,
  type Box,
  type CanvasCard,
  type CanvasDoc,
  type CanvasView,
} from '@/lib/canvas'
import { cx } from '@/lib/cx'
import { isWebUrl } from '@/lib/url'

const VIEW_SETTLE_MS = 400
const DRAG_SLOP = 3
const LINE_PX = 16
const FAR_ZOOM = 0.35

interface GestureEvent extends UIEvent {
  scale: number
  clientX: number
  clientY: number
}

interface CanvasSurfaceProps {
  doc: CanvasDoc
  onChange: (doc: CanvasDoc) => void
  onView: (view: CanvasView) => void
  onUndo: () => void
  onRedo: () => void
}

type Selection = ReadonlySet<string>

const NONE: Selection = new Set()
const transformOf = (x: number, y: number) => `translate3d(${x}px,${y}px,0)`
const viewTransform = ({ x, y, k }: CanvasView) => `translate3d(${x}px,${y}px,0) scale(${k})`
const sameSet = (a: Selection, b: Selection) => a.size === b.size && [...a].every((id) => b.has(id))
const typing = (target: EventTarget | null) =>
  target instanceof Element && target.closest('input, textarea') !== null

function openLink(href: string) {
  if (isWebUrl(href)) window.open(href, '_blank', 'noopener,noreferrer')
}

export function CanvasSurface({ doc, onChange, onView, onUndo, onRedo }: CanvasSurfaceProps) {
  const viewportRef = useRef<HTMLDivElement>(null)
  const layerRef = useRef<HTMLDivElement>(null)
  const zoomRef = useRef<HTMLButtonElement>(null)
  const view = useRef<CanvasView>(doc.view)
  const docRef = useRef(doc)
  docRef.current = doc
  const props = useRef({ onChange, onView, onUndo, onRedo })
  props.current = { onChange, onView, onUndo, onRedo }
  const cardEls = useRef(new Map<string, HTMLElement>())
  const dragged = useRef<{ bases: Map<string, CanvasCard>; dx: number; dy: number } | null>(null)
  const frame = useRef(0)
  const viewTimer = useRef(0)
  const spaceHeld = useRef(false)
  const [selected, setSelectedState] = useState<Selection>(NONE)
  const selectedRef = useRef(selected)
  const [editing, setEditing] = useState<string | null>(null)
  const [draft, setDraft] = useState<CanvasCard | null>(null)
  const [marquee, setMarquee] = useState<Box | null>(null)
  const [panning, setPanning] = useState(false)

  const select = (next: Selection) => {
    if (sameSet(next, selectedRef.current)) return
    selectedRef.current = next
    setSelectedState(next)
  }

  const paint = () => {
    frame.current = 0
    const { k } = view.current
    const layer = layerRef.current
    if (layer) {
      layer.style.transform = viewTransform(view.current)
      layer.classList.toggle('is-far', k < FAR_ZOOM)
    }
    if (zoomRef.current) zoomRef.current.textContent = `${Math.round(k * 100)}%`
    const d = dragged.current
    if (d) {
      for (const [id, base] of d.bases) {
        const el = cardEls.current.get(id)
        if (el) el.style.transform = transformOf(base.x + d.dx, base.y + d.dy)
      }
    }
  }
  const schedule = () => {
    if (frame.current === 0) frame.current = requestAnimationFrame(paint)
  }
  const settleView = () => {
    window.clearTimeout(viewTimer.current)
    viewTimer.current = window.setTimeout(() => props.current.onView(view.current), VIEW_SETTLE_MS)
  }
  const localPoint = (clientX: number, clientY: number) => {
    const rect = viewportRef.current?.getBoundingClientRect()
    return { x: clientX - (rect?.left ?? 0), y: clientY - (rect?.top ?? 0) }
  }
  const zoomBy = (factor: number) => {
    const rect = viewportRef.current?.getBoundingClientRect()
    view.current = zoomAt(view.current, (rect?.width ?? 0) / 2, (rect?.height ?? 0) / 2, factor)
    schedule()
    settleView()
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
    schedule()
    return () => {
      el.removeEventListener('wheel', onWheel)
      el.removeEventListener('gesturestart', onGestureStart)
      el.removeEventListener('gesturechange', onGestureChange)
      window.clearTimeout(viewTimer.current)
      cancelAnimationFrame(frame.current)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (typing(e.target)) return
      const mod = e.metaKey || e.ctrlKey
      if (e.key === ' ') {
        e.preventDefault()
        if (!spaceHeld.current) {
          spaceHeld.current = true
          setPanning(true)
        }
      } else if (e.key === 'Escape') {
        select(NONE)
      } else if ((e.key === 'Delete' || e.key === 'Backspace') && selectedRef.current.size > 0) {
        e.preventDefault()
        props.current.onChange(removeCards(docRef.current, selectedRef.current))
        select(NONE)
      } else if (mod && e.key.toLowerCase() === 'a') {
        e.preventDefault()
        select(new Set(docRef.current.cards.map((c) => c.id)))
      } else if (mod && e.key.toLowerCase() === 'z') {
        e.preventDefault()
        if (e.shiftKey) props.current.onRedo()
        else props.current.onUndo()
      }
    }
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key === ' ') {
        spaceHeld.current = false
        setPanning(false)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    window.addEventListener('keyup', onKeyUp)
    return () => {
      window.removeEventListener('keydown', onKeyDown)
      window.removeEventListener('keyup', onKeyUp)
    }
  }, [])

  const pan = (e: ReactPointerEvent<HTMLDivElement>) => {
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

  const boxSelect = (e: ReactPointerEvent<HTMLDivElement>) => {
    const keep = e.shiftKey || e.metaKey ? selectedRef.current : NONE
    const start = localPoint(e.clientX, e.clientY)
    const boxes = docRef.current.cards.map((c) => ({
      id: c.id,
      x: c.x,
      y: c.y,
      w: c.w,
      h: cardEls.current.get(c.id)?.offsetHeight ?? 0,
    }))
    select(keep)
    let latest = start
    let pending = 0
    const update = () => {
      pending = 0
      const box = boxBetween(start.x, start.y, latest.x, latest.y)
      setMarquee(box)
      const a = toCanvas(view.current, box.x, box.y)
      const b = toCanvas(view.current, box.x + box.w, box.y + box.h)
      const hits = cardsTouching(boxes, boxBetween(a.x, a.y, b.x, b.y))
      select(new Set([...keep, ...hits]))
    }
    const move = (ev: PointerEvent) => {
      latest = localPoint(ev.clientX, ev.clientY)
      if (pending === 0) pending = requestAnimationFrame(update)
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      cancelAnimationFrame(pending)
      setMarquee(null)
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  const onBackgroundDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button === 1 || (e.button === 0 && spaceHeld.current)) {
      e.preventDefault()
      pan(e)
      return
    }
    if (e.button !== 0 || (e.target as Element).closest('.canvas-card')) return
    boxSelect(e)
  }

  const onBackgroundDoubleClick = (e: ReactMouseEvent<HTMLDivElement>) => {
    if ((e.target as Element).closest('.canvas-card')) return
    const p = localPoint(e.clientX, e.clientY)
    const at = toCanvas(view.current, p.x, p.y)
    select(NONE)
    setDraft(newCard(at.x, at.y))
  }

  const commitDraft = (text: string) => {
    const card = draft
    setDraft(null)
    if (card && text.trim() !== '') {
      props.current.onChange(addCard(docRef.current, { ...card, text }))
      select(new Set([card.id]))
    }
  }

  const handlers = useRef({
    register(id: string, el: HTMLElement | null) {
      if (el) cardEls.current.set(id, el)
      else cardEls.current.delete(id)
    },
    down(card: CanvasCard, e: ReactPointerEvent<HTMLElement>) {
      if (e.button !== 0 || spaceHeld.current) return
      e.stopPropagation()
      const current = selectedRef.current
      if (e.shiftKey || e.metaKey) {
        const next = new Set(current)
        if (next.has(card.id)) next.delete(card.id)
        else next.add(card.id)
        select(next)
        return
      }
      const wasSelected = current.has(card.id)
      const group = wasSelected ? current : new Set([card.id])
      if (!wasSelected) select(group)
      const bases = new Map(
        docRef.current.cards.filter((c) => group.has(c.id)).map((c) => [c.id, c] as const),
      )
      const start = { px: e.clientX, py: e.clientY }
      let moving = false
      const move = (ev: PointerEvent) => {
        const dx = ev.clientX - start.px
        const dy = ev.clientY - start.py
        if (!moving && Math.hypot(dx, dy) < DRAG_SLOP) return
        moving = true
        const k = view.current.k
        dragged.current = { bases, dx: dx / k, dy: dy / k }
        schedule()
      }
      const up = () => {
        window.removeEventListener('pointermove', move)
        window.removeEventListener('pointerup', up)
        const d = dragged.current
        dragged.current = null
        if (moving && d) props.current.onChange(moveCards(docRef.current, new Set(d.bases.keys()), d.dx, d.dy))
        else if (wasSelected && group.size > 1) select(new Set([card.id]))
      }
      window.addEventListener('pointermove', move)
      window.addEventListener('pointerup', up)
    },
    edit(id: string) {
      select(new Set([id]))
      setEditing(id)
    },
    commit(id: string, text: string) {
      setEditing(null)
      const current = docRef.current.cards.find((c) => c.id === id)
      if (!current) return
      if (text.trim() === '') props.current.onChange(removeCard(docRef.current, id))
      else if (text !== current.text) props.current.onChange(updateCard(docRef.current, id, { text }))
    },
  }).current

  return (
    <div
      className={cx('canvas-viewport', { 'is-panning': panning })}
      ref={viewportRef}
      onPointerDown={onBackgroundDown}
      onDoubleClick={onBackgroundDoubleClick}
    >
      <div className="canvas-layer" ref={layerRef} style={{ transform: viewTransform(view.current) }}>
        {doc.cards.map((card) => (
          <Card
            key={card.id}
            card={card}
            selected={selected.has(card.id)}
            editing={editing === card.id}
            handlers={handlers}
          />
        ))}
        {draft && (
          <div
            className="canvas-card is-selected is-editing"
            style={{ width: draft.w, transform: transformOf(draft.x, draft.y) }}
          >
            <CardEditor text="" onDone={commitDraft} />
          </div>
        )}
      </div>
      {marquee && (
        <div
          className="canvas-marquee"
          style={{ left: marquee.x, top: marquee.y, width: marquee.w, height: marquee.h }}
        />
      )}
      {doc.cards.length === 0 && !draft && <p className="canvas-hint">Double-click anywhere to add a card</p>}
      <button
        ref={zoomRef}
        type="button"
        className="canvas-zoom"
        title="Back to 100%"
        onPointerDown={(e) => e.stopPropagation()}
        onClick={() => zoomBy(1 / view.current.k)}
      />
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
      onPointerDown={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        if (e.key === 'Escape') e.currentTarget.blur()
      }}
    />
  )
}
