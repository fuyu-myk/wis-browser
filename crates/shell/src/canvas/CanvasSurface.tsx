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
  addEdge,
  ARROW_LENGTH,
  boxBetween,
  cardsTouching,
  edgeGeometry,
  moveCards,
  newCard,
  pointerArrow,
  removeCard,
  removeCards,
  removeEdge,
  toCanvas,
  updateCard,
  zoomAt,
  type Box,
  type CanvasCard,
  type CanvasDoc,
  type CanvasEdge,
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
type Side = 'top' | 'right' | 'bottom' | 'left'
interface Dragged {
  bases: Map<string, CanvasCard>
  edges: CanvasEdge[]
  dx: number
  dy: number
}

const SIDES: readonly Side[] = ['top', 'right', 'bottom', 'left']

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
  const dragged = useRef<Dragged | null>(null)
  const heights = useRef(new Map<string, number>())
  const edgeEls = useRef(new Map<string, SVGPathElement[]>())
  const [, setLayout] = useState(0)
  const resizes = useRef<ResizeObserver | null>(null)
  if (resizes.current === null && typeof ResizeObserver !== 'undefined') {
    resizes.current = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const id = (entry.target as HTMLElement).dataset.card
        if (id) heights.current.set(id, (entry.target as HTMLElement).offsetHeight)
      }
      setLayout((n) => n + 1)
    })
  }
  const frame = useRef(0)
  const viewTimer = useRef(0)
  const spaceHeld = useRef(false)
  const [selected, setSelectedState] = useState<Selection>(NONE)
  const selectedRef = useRef(selected)
  const [editing, setEditing] = useState<string | null>(null)
  const [draft, setDraft] = useState<CanvasCard | null>(null)
  const [marquee, setMarquee] = useState<Box | null>(null)
  const [selectedEdge, setSelectedEdge] = useState<string | null>(null)
  const [linking, setLinking] = useState<{ sx: number; sy: number; ox: number; oy: number; x: number; y: number } | null>(
    null,
  )
  const boxOf = (card: CanvasCard, dx = 0, dy = 0): Box => ({
    x: card.x + dx,
    y: card.y + dy,
    w: card.w,
    h: heights.current.get(card.id) ?? 0,
  })
  const [panning, setPanning] = useState(false)

  const selectedEdgeRef = useRef(selectedEdge)
  selectedEdgeRef.current = selectedEdge
  const cardsById = new Map(doc.cards.map((c) => [c.id, c]))
  const select = (next: Selection) => {
    if (next.size > 0) setSelectedEdge(null)
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
      const cards = new Map(docRef.current.cards.map((c) => [c.id, c]))
      const placed = (id: string) => {
        const card = cards.get(id)
        if (!card) return null
        return d.bases.has(id) ? boxOf(card, d.dx, d.dy) : boxOf(card)
      }
      for (const edge of d.edges) {
        const a = placed(edge.from)
        const b = placed(edge.to)
        if (!a || !b) continue
        const path = edgeGeometry(a, b).d
        for (const el of edgeEls.current.get(edge.id) ?? []) el.setAttribute('d', path)
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
        setSelectedEdge(null)
      } else if ((e.key === 'Delete' || e.key === 'Backspace') && selectedEdgeRef.current !== null) {
        e.preventDefault()
        props.current.onChange(removeEdge(docRef.current, selectedEdgeRef.current))
        setSelectedEdge(null)
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
    setSelectedEdge(null)
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
      const old = cardEls.current.get(id)
      if (old && old !== el) resizes.current?.unobserve(old)
      if (el) {
        cardEls.current.set(id, el)
        resizes.current?.observe(el)
      } else {
        cardEls.current.delete(id)
        heights.current.delete(id)
      }
    },
    link(card: CanvasCard, side: Side, e: ReactPointerEvent<HTMLElement>) {
      if (e.button !== 0) return
      e.stopPropagation()
      const box = boxOf(card)
      const [sx, sy, ox, oy] = {
        top: [box.x + box.w / 2, box.y, 0, -1],
        right: [box.x + box.w, box.y + box.h / 2, 1, 0],
        bottom: [box.x + box.w / 2, box.y + box.h, 0, 1],
        left: [box.x, box.y + box.h / 2, -1, 0],
      }[side]
      setLinking({ sx, sy, ox, oy, x: sx, y: sy })
      let latest = { x: e.clientX, y: e.clientY }
      let pending = 0
      const update = () => {
        pending = 0
        const p = localPoint(latest.x, latest.y)
        const at = toCanvas(view.current, p.x, p.y)
        setLinking({ sx, sy, ox, oy, x: at.x, y: at.y })
      }
      const move = (ev: PointerEvent) => {
        latest = { x: ev.clientX, y: ev.clientY }
        if (pending === 0) pending = requestAnimationFrame(update)
      }
      const up = (ev: PointerEvent) => {
        window.removeEventListener('pointermove', move)
        window.removeEventListener('pointerup', up)
        cancelAnimationFrame(pending)
        setLinking(null)
        const target = document.elementFromPoint(ev.clientX, ev.clientY)?.closest<HTMLElement>('[data-card]')
        const to = target?.dataset.card
        if (to && to !== card.id) props.current.onChange(addEdge(docRef.current, card.id, to))
      }
      window.addEventListener('pointermove', move)
      window.addEventListener('pointerup', up)
    },
    pickEdge(id: string, e: ReactPointerEvent<SVGPathElement>) {
      if (e.button !== 0) return
      e.stopPropagation()
      select(NONE)
      setSelectedEdge(id)
    },
    registerEdge(id: string, el: SVGPathElement | null) {
      if (!el) return
      const list = (edgeEls.current.get(id) ?? []).filter((p) => p.isConnected)
      if (!list.includes(el)) list.push(el)
      edgeEls.current.set(id, list)
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
      const edges = docRef.current.edges.filter((edge) => group.has(edge.from) || group.has(edge.to))
      const start = { px: e.clientX, py: e.clientY }
      let moving = false
      const move = (ev: PointerEvent) => {
        const dx = ev.clientX - start.px
        const dy = ev.clientY - start.py
        if (!moving && Math.hypot(dx, dy) < DRAG_SLOP) return
        moving = true
        const k = view.current.k
        dragged.current = { bases, edges, dx: dx / k, dy: dy / k }
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
        <svg className="canvas-edges" width="1" height="1" aria-hidden>
          <defs>
            {(['canvas-arrow', 'canvas-arrow-on'] as const).map((id) => (
              <marker
                key={id}
                id={id}
                viewBox="0 0 10 8"
                refX="0"
                refY="4"
                markerUnits="userSpaceOnUse"
                markerWidth={ARROW_LENGTH}
                markerHeight={ARROW_LENGTH * 0.8}
                orient="auto"
              >
                <path
                  d="M0,0 L10,4 L0,8 z"
                  className={cx('canvas-arrowhead', { 'is-selected': id === 'canvas-arrow-on' })}
                />
              </marker>
            ))}
          </defs>
          {doc.edges.map((edge) => {
            const a = cardsById.get(edge.from)
            const b = cardsById.get(edge.to)
            if (!a || !b) return null
            const d = edgeGeometry(boxOf(a), boxOf(b)).d
            const on = selectedEdge === edge.id
            return (
              <g key={edge.id}>
                <path
                  ref={(el) => handlers.registerEdge(edge.id, el)}
                  className="canvas-edge-hit"
                  d={d}
                  onPointerDown={(e) => handlers.pickEdge(edge.id, e)}
                />
                <path
                  ref={(el) => handlers.registerEdge(edge.id, el)}
                  className={cx('canvas-edge', { 'is-selected': on })}
                  d={d}
                  markerEnd={on ? 'url(#canvas-arrow-on)' : 'url(#canvas-arrow)'}
                />
              </g>
            )
          })}
          {linking && (
            <path
              className="canvas-link"
              d={pointerArrow({ x: linking.sx, y: linking.sy }, { x: linking.ox, y: linking.oy }, linking)}
              markerEnd="url(#canvas-arrow-on)"
            />
          )}
        </svg>
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
    link: (card: CanvasCard, side: Side, e: ReactPointerEvent<HTMLElement>) => void
    down: (card: CanvasCard, e: ReactPointerEvent<HTMLElement>) => void
    edit: (id: string) => void
    commit: (id: string, text: string) => void
  }
}

const Card = memo(function Card({ card, selected, editing, handlers }: CardProps) {
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
