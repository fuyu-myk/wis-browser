import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MutableRefObject,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react'

import {
  addCard,
  addEdge,
  addFrame,
  boxBetween,
  canGroup,
  canUngroup,
  cardsInFrame,
  cardsTouching,
  expandToGroups,
  frameAround,
  framesInside,
  groupCards,
  groupsIn,
  HTML_CARD_HEIGHT,
  MAX_CARD_HEIGHT,
  MAX_CARD_WIDTH,
  MAX_FRAME_SIZE,
  MIN_CARD_HEIGHT,
  MIN_CARD_WIDTH,
  MIN_FRAME_H,
  MIN_FRAME_W,
  moveFrames,
  newCard,
  newFrame,
  newHtmlCard,
  removeCard,
  removeCards,
  removeEdge,
  removeFrame,
  renameFrame,
  resizeCard,
  resizeFrame,
  tidyName,
  toCanvas,
  ungroupCards,
  updateCard,
  zoomAt,
  type Box,
  type CanvasCard,
  type CanvasDoc,
  type CanvasView,
} from '@/lib/canvas'
import { estimateHeight } from '@/lib/canvasAgent'
import { cx } from '@/lib/cx'

import { Card, CardEditor, type CardHandlers, type Side } from './CanvasCard'
import { CanvasEdges, type Linking } from './CanvasEdges'
import { Frame, type FrameHandlers } from './CanvasFrame'
import { CanvasGroups, type GroupOutline } from './CanvasGroups'
import { canvasMenuItems, CanvasMenu, type CanvasCommands } from './CanvasMenu'
import { follow, transformOf } from './dom'
import { paintDrag, type Dragged } from './paintDrag'
import { useAwakeCards } from './useAwakeCards'
import { useCardHealth, type CardPulse } from './useCardHealth'
import { useLiveCard } from './useLiveCard'
import { useViewGestures } from './useViewGestures'

const VIEW_SETTLE_MS = 400
const DRAG_SLOP = 3
const FAR_ZOOM = 0.35
const NEW_FRAME_W = 480
const NEW_FRAME_H = 320

interface CanvasSurfaceProps {
  doc: CanvasDoc
  onChange: (doc: CanvasDoc) => void
  onView: (view: CanvasView) => void
  onUndo: () => void
  onRedo: () => void
  pulse: CardPulse
  handle?: MutableRefObject<SurfaceHandle | null>
}

/** What the canvas agent reads from, and does to, the surface. */
export interface SurfaceHandle {
  selection: () => string[]
  view: () => Box
  heightOf: (card: CanvasCard) => number
  select: (ids: readonly string[]) => void
}

type Selection = ReadonlySet<string>
type Picked = { kind: 'edge' | 'frame'; id: string } | null
type Point = { x: number; y: number }
type Sizing = { kind: 'frame' | 'card'; id: string; w: number; h: number }
interface Limits {
  minW: number
  maxW: number
  minH: number
  maxH: number
}
const FRAME_LIMITS: Limits = { minW: MIN_FRAME_W, maxW: MAX_FRAME_SIZE, minH: MIN_FRAME_H, maxH: MAX_FRAME_SIZE }
const CARD_LIMITS: Limits = { minW: MIN_CARD_WIDTH, maxW: MAX_CARD_WIDTH, minH: MIN_CARD_HEIGHT, maxH: MAX_CARD_HEIGHT }

const NONE: Selection = new Set()
const viewTransform = ({ x, y, k }: CanvasView) => `translate3d(${x}px,${y}px,0) scale(${k})`
const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n))
const sameSet = (a: Selection, b: Selection) => a.size === b.size && [...a].every((id) => b.has(id))
const typing = (target: EventTarget | null) =>
  target instanceof Element && target.closest('input, textarea') !== null

export function CanvasSurface({ doc, onChange, onView, onUndo, onRedo, pulse, handle }: CanvasSurfaceProps) {
  const viewportRef = useRef<HTMLDivElement>(null)
  const layerRef = useRef<HTMLDivElement>(null)
  const zoomRef = useRef<HTMLButtonElement>(null)
  const view = useRef<CanvasView>(doc.view)
  const docRef = useRef(doc)
  docRef.current = doc
  const props = useRef({ onChange, onView, onUndo, onRedo })
  props.current = { onChange, onView, onUndo, onRedo }
  const cardEls = useRef(new Map<string, HTMLElement>())
  const frameEls = useRef(new Map<string, HTMLElement>())
  const groupEls = useRef(new Map<string, GroupOutline>())
  const dragged = useRef<Dragged | null>(null)
  const sizing = useRef<Sizing | null>(null)
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
  const [picked, setPicked] = useState<Picked>(null)
  const pickedRef = useRef(picked)
  pickedRef.current = picked
  const [editing, setEditing] = useState<string | null>(null)
  const [live, setLive] = useLiveCard(viewportRef, cardEls)
  const [renaming, setRenaming] = useState<string | null>(null)
  const [draft, setDraft] = useState<CanvasCard | null>(null)
  const [marquee, setMarquee] = useState<Box | null>(null)
  const [linking, setLinking] = useState<Linking | null>(null)
  const [menu, setMenu] = useState<{ x: number; y: number; at: Point; html?: string } | null>(null)
  const [panning, setPanning] = useState(false)
  const [far, setFar] = useState(doc.view.k < FAR_ZOOM)
  const awakeCards = useAwakeCards(viewportRef)
  const wanted = !far && doc.cards.some((c) => c.kind === 'html' && awakeCards.awake.has(c.id))
  const health = useCardHealth(pulse, wanted)
  const paused = (card: CanvasCard) =>
    card.kind === 'html' && !(health.running && !far && awakeCards.awake.has(card.id))
  const boxOf = (card: CanvasCard, dx = 0, dy = 0): Box => ({
    x: card.x + dx,
    y: card.y + dy,
    w: card.w,
    h: heights.current.get(card.id) ?? 0,
  })

  const cardsById = new Map(doc.cards.map((c) => [c.id, c]))
  const boxOfId = (id: string) => {
    const card = cardsById.get(id)
    return card ? boxOf(card) : null
  }
  const frames = useMemo(() => [...doc.frames].sort((a, b) => b.w * b.h - a.w * a.h), [doc.frames])
  const select = (next: Selection) => {
    if (next.size > 0) setPicked(null)
    if (sameSet(next, selectedRef.current)) return
    selectedRef.current = next
    setSelectedState(next)
  }
  const pick = (next: Picked) => {
    if (next) select(NONE)
    setPicked(next)
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
    const s = sizing.current
    const sized = s && (s.kind === 'card' ? cardEls : frameEls).current.get(s.id)
    if (s && sized) {
      sized.style.width = `${s.w}px`
      sized.style.height = `${s.h}px`
    }
    const d = dragged.current
    if (!d) return
    paintDrag(
      d,
      docRef.current.cards,
      { cards: cardEls.current, frames: frameEls.current, groups: groupEls.current, edges: edgeEls.current },
      boxOf,
    )
  }
  const schedule = () => {
    if (frame.current === 0) frame.current = requestAnimationFrame(paint)
  }
  const settleView = () => {
    window.clearTimeout(viewTimer.current)
    viewTimer.current = window.setTimeout(() => {
      props.current.onView(view.current)
      setFar(view.current.k < FAR_ZOOM)
    }, VIEW_SETTLE_MS)
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

  const drag = (
    e: ReactPointerEvent<HTMLElement>,
    cards: ReadonlySet<string>,
    frameIds: ReadonlySet<string>,
    onClick?: () => void,
  ) => {
    const current = docRef.current
    const bases = new Map(current.cards.filter((c) => cards.has(c.id)).map((c) => [c.id, c] as const))
    const frameBases = new Map(current.frames.filter((f) => frameIds.has(f.id)).map((f) => [f.id, f] as const))
    const edges = current.edges.filter((edge) => cards.has(edge.from) || cards.has(edge.to))
    const start = { px: e.clientX, py: e.clientY }
    let moving = false
    follow(
      (ev) => {
        const dx = ev.clientX - start.px
        const dy = ev.clientY - start.py
        if (!moving && Math.hypot(dx, dy) < DRAG_SLOP) return
        moving = true
        const k = view.current.k
        dragged.current = { bases, frames: frameBases, edges, dx: dx / k, dy: dy / k }
        schedule()
      },
      () => {
        const d = dragged.current
        dragged.current = null
        if (moving && d) props.current.onChange(moveFrames(docRef.current, frameIds, cards, d.dx, d.dy))
        else onClick?.()
      },
    )
  }

  const resizeFrom = (
    e: ReactPointerEvent<HTMLElement>,
    target: Omit<Sizing, 'kind'>,
    kind: Sizing['kind'],
    limits: Limits,
    commit: (doc: CanvasDoc, w: number, h: number) => CanvasDoc,
  ) => {
    const start = { px: e.clientX, py: e.clientY }
    follow(
      (ev) => {
        const k = view.current.k
        sizing.current = {
          kind,
          id: target.id,
          w: clamp(target.w + (ev.clientX - start.px) / k, limits.minW, limits.maxW),
          h: clamp(target.h + (ev.clientY - start.py) / k, limits.minH, limits.maxH),
        }
        schedule()
      },
      () => {
        const s = sizing.current
        sizing.current = null
        if (s) props.current.onChange(commit(docRef.current, s.w, s.h))
      },
    )
  }

  const placeFrame = (box: Box) => {
    const made = newFrame(box)
    props.current.onChange(addFrame(docRef.current, made))
    pick({ kind: 'frame', id: made.id })
    setRenaming(made.id)
  }

  const commands = useRef<CanvasCommands>({
    group() {
      const current = docRef.current
      if (canGroup(current, selectedRef.current)) props.current.onChange(groupCards(current, selectedRef.current))
    },
    ungroup() {
      const current = docRef.current
      if (canUngroup(current, selectedRef.current)) props.current.onChange(ungroupCards(current, selectedRef.current))
    },
    frameSelection() {
      const boxes = docRef.current.cards.filter((c) => selectedRef.current.has(c.id)).map((c) => boxOf(c))
      if (boxes.length > 0) placeFrame(frameAround(boxes))
    },
    newFrame(at) {
      placeFrame({ x: at.x, y: at.y, w: NEW_FRAME_W, h: NEW_FRAME_H })
    },
    newCard(at) {
      select(NONE)
      setDraft(newCard(at.x, at.y))
    },
    newHtmlCard(at) {
      const made = newHtmlCard(at.x, at.y)
      props.current.onChange(addCard(docRef.current, made))
      select(new Set([made.id]))
      setEditing(made.id)
    },
    editCard(id) {
      select(expandToGroups(docRef.current, new Set([id])))
      setLive(null)
      setEditing(id)
    },
    remove() {
      const current = docRef.current
      const target = pickedRef.current
      if (target?.kind === 'edge') props.current.onChange(removeEdge(current, target.id))
      else if (target?.kind === 'frame') props.current.onChange(removeFrame(current, target.id))
      else if (selectedRef.current.size > 0) props.current.onChange(removeCards(current, selectedRef.current))
      select(NONE)
      setPicked(null)
    },
  }).current

  useEffect(() => {
    if (!handle) return
    handle.current = {
      selection: () => [...selectedRef.current],
      view: () => {
        const a = toCanvas(view.current, 0, 0)
        const b = toCanvas(view.current, window.innerWidth, window.innerHeight)
        return { x: a.x, y: a.y, w: b.x - a.x, h: b.y - a.y }
      },
      heightOf: (card) => heights.current.get(card.id) ?? estimateHeight(card),
      select: (ids) => select(new Set(ids)),
    }
    return () => {
      handle.current = null
    }
  }, [handle])

  useViewGestures(viewportRef, view, () => {
    schedule()
    settleView()
  })
  useEffect(() => {
    schedule()
    return () => {
      window.clearTimeout(viewTimer.current)
      cancelAnimationFrame(frame.current)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (typing(e.target)) return
      const mod = e.metaKey || e.ctrlKey
      const erase = e.key === 'Delete' || e.key === 'Backspace'
      if (e.key === ' ') {
        e.preventDefault()
        if (!spaceHeld.current) {
          spaceHeld.current = true
          setPanning(true)
        }
      } else if (e.key === 'Escape') {
        select(NONE)
        setPicked(null)
        setLive(null)
      } else if (erase && (pickedRef.current !== null || selectedRef.current.size > 0)) {
        e.preventDefault()
        commands.remove()
      } else if (mod && e.code === 'KeyG' && selectedRef.current.size > 0) {
        e.preventDefault()
        if (e.altKey) commands.frameSelection()
        else if (e.shiftKey) commands.ungroup()
        else commands.group()
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
  }, [commands, setLive])

  const liveCard = live === null ? undefined : cardsById.get(live)
  const liveGone = live !== null && (!liveCard || paused(liveCard))
  useEffect(() => {
    if (liveGone) setLive(null)
  }, [liveGone, setLive])

  const pan = (e: ReactPointerEvent<HTMLDivElement>) => {
    const start = { px: e.clientX, py: e.clientY, x: view.current.x, y: view.current.y }
    follow(
      (ev) => {
        view.current = { ...view.current, x: start.x + ev.clientX - start.px, y: start.y + ev.clientY - start.py }
        schedule()
      },
      settleView,
    )
  }

  const boxSelect = (e: ReactPointerEvent<HTMLDivElement>) => {
    const keep = e.shiftKey ? selectedRef.current : NONE
    const deep = e.metaKey
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
      const picked = new Set([...keep, ...hits])
      select(deep ? picked : expandToGroups(docRef.current, picked))
    }
    follow(
      (ev) => {
        latest = localPoint(ev.clientX, ev.clientY)
        if (pending === 0) pending = requestAnimationFrame(update)
      },
      () => {
        cancelAnimationFrame(pending)
        setMarquee(null)
      },
    )
  }

  const onBackgroundDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button === 1 || (e.button === 0 && spaceHeld.current)) {
      e.preventDefault()
      pan(e)
      return
    }
    if (e.button !== 0 || (e.target as Element).closest('.canvas-card')) return
    setPicked(null)
    boxSelect(e)
  }

  const canvasPoint = (clientX: number, clientY: number) => {
    const p = localPoint(clientX, clientY)
    return toCanvas(view.current, p.x, p.y)
  }

  const onBackgroundDoubleClick = (e: ReactMouseEvent<HTMLDivElement>) => {
    if ((e.target as Element).closest('.canvas-card, .canvas-frame')) return
    commands.newCard(canvasPoint(e.clientX, e.clientY))
  }

  const onContextMenu = (e: ReactMouseEvent<HTMLDivElement>) => {
    if (typing(e.target)) return
    e.preventDefault()
    const target = e.target as Element
    const card = target.closest<HTMLElement>('[data-card]')?.dataset.card
    const frameId = target.closest<HTMLElement>('[data-frame]')?.dataset.frame
    const edge = target.closest<SVGElement>('[data-edge]')?.dataset.edge
    if (card && !selectedRef.current.has(card))
      select(e.metaKey ? new Set([card]) : expandToGroups(docRef.current, new Set([card])))
    else if (frameId) pick({ kind: 'frame', id: frameId })
    else if (edge) pick({ kind: 'edge', id: edge })
    const html = docRef.current.cards.find((c) => c.id === card)?.kind === 'html' ? card : undefined
    setMenu({ x: e.clientX, y: e.clientY, at: canvasPoint(e.clientX, e.clientY), html })
  }
  const closeMenu = useCallback(() => setMenu(null), [])

  const commitDraft = (text: string) => {
    const card = draft
    setDraft(null)
    if (card && text.trim() !== '') {
      props.current.onChange(addCard(docRef.current, { ...card, text }))
      select(new Set([card.id]))
    }
  }

  const handlers = useRef<CardHandlers>({
    register(id, el) {
      const old = cardEls.current.get(id)
      if (old && old !== el) resizes.current?.unobserve(old)
      if (el) {
        cardEls.current.set(id, el)
        resizes.current?.observe(el)
        if (el.classList.contains('is-html')) awakeCards.watch(el)
      } else {
        if (old) awakeCards.unwatch(old)
        cardEls.current.delete(id)
        heights.current.delete(id)
      }
    },
    link(card: CanvasCard, side: Side, e) {
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
        const at = canvasPoint(latest.x, latest.y)
        setLinking({ sx, sy, ox, oy, x: at.x, y: at.y })
      }
      follow(
        (ev) => {
          latest = { x: ev.clientX, y: ev.clientY }
          if (pending === 0) pending = requestAnimationFrame(update)
        },
        (ev) => {
          cancelAnimationFrame(pending)
          setLinking(null)
          const target = document.elementFromPoint(ev.clientX, ev.clientY)?.closest<HTMLElement>('[data-card]')
          const to = target?.dataset.card
          if (to && to !== card.id) props.current.onChange(addEdge(docRef.current, card.id, to))
        },
      )
    },
    down(card, e) {
      if (e.button !== 0 || spaceHeld.current) return
      e.stopPropagation()
      const own = e.metaKey ? new Set([card.id]) : expandToGroups(docRef.current, new Set([card.id]))
      const current = selectedRef.current
      const wasSelected = current.has(card.id)
      if (e.shiftKey) {
        const next = new Set(current)
        for (const id of own) {
          if (wasSelected) next.delete(id)
          else next.add(id)
        }
        select(next)
        return
      }
      const moving = wasSelected && !e.metaKey ? current : own
      if (moving !== current) select(moving)
      drag(e, moving, NONE, () => {
        if (moving === current && !sameSet(current, own)) select(own)
      })
    },
    edit(id) {
      commands.editCard(id)
    },
    use(id) {
      select(expandToGroups(docRef.current, new Set([id])))
      setEditing(null)
      setLive(id)
    },
    resize(target, e) {
      if (e.button !== 0) return
      e.stopPropagation()
      setLive(null)
      select(new Set([target.id]))
      const box = { id: target.id, w: target.w, h: target.h ?? HTML_CARD_HEIGHT }
      resizeFrom(e, box, 'card', CARD_LIMITS, (doc, w, h) => resizeCard(doc, target.id, w, h))
    },
    commit(id, text) {
      setEditing(null)
      const current = docRef.current.cards.find((c) => c.id === id)
      if (!current) return
      if (text.trim() === '') props.current.onChange(removeCard(docRef.current, id))
      else if (text !== current.text) props.current.onChange(updateCard(docRef.current, id, { text }))
    },
  }).current

  const edgeHandlers = useRef({
    pick(id: string, e: ReactPointerEvent<SVGPathElement>) {
      if (e.button !== 0) return
      e.stopPropagation()
      pick({ kind: 'edge', id })
    },
    register(id: string, el: SVGPathElement | null) {
      if (!el) return
      const list = (edgeEls.current.get(id) ?? []).filter((p) => p.isConnected)
      if (!list.includes(el)) list.push(el)
      edgeEls.current.set(id, list)
    },
  }).current

  const frameHandlers = useRef<FrameHandlers>({
    register(id, el) {
      if (el) frameEls.current.set(id, el)
      else frameEls.current.delete(id)
    },
    down(target, e) {
      if (e.button !== 0 || spaceHeld.current) return
      e.stopPropagation()
      pick({ kind: 'frame', id: target.id })
      const current = docRef.current
      const boxes = current.cards.map((c) => ({ ...boxOf(c), id: c.id }))
      const members = expandToGroups(current, new Set(cardsInFrame(boxes, target)))
      drag(e, members, new Set([target.id, ...framesInside(current.frames, target)]))
    },
    resize(target, e) {
      if (e.button !== 0) return
      e.stopPropagation()
      pick({ kind: 'frame', id: target.id })
      resizeFrom(e, target, 'frame', FRAME_LIMITS, (doc, w, h) => resizeFrame(doc, target.id, w, h))
    },
    rename(id) {
      pick({ kind: 'frame', id })
      setRenaming(id)
    },
    renamed(id, title) {
      setRenaming(null)
      const before = docRef.current.frames.find((f) => f.id === id)
      const tidy = title === null ? '' : tidyName(title)
      if (before && tidy !== '' && tidy !== before.title) props.current.onChange(renameFrame(docRef.current, id, tidy))
    },
  }).current

  const registerGroup = (group: string, outline: GroupOutline | null) => {
    if (outline) groupEls.current.set(group, outline)
    else groupEls.current.delete(group)
  }

  return (
    <>
      <div
        className={cx('canvas-viewport', { 'is-panning': panning })}
        ref={viewportRef}
        tabIndex={-1}
        onPointerDown={onBackgroundDown}
        onDoubleClick={onBackgroundDoubleClick}
        onContextMenu={onContextMenu}
      >
        <div className="canvas-layer" ref={layerRef} style={{ transform: viewTransform(view.current) }}>
          {frames.map((f) => (
            <Frame
              key={f.id}
              frame={f}
              selected={picked?.kind === 'frame' && picked.id === f.id}
              renaming={renaming === f.id}
              handlers={frameHandlers}
            />
          ))}
          <CanvasEdges
            edges={doc.edges}
            boxOf={boxOfId}
            selected={picked?.kind === 'edge' ? picked.id : null}
            linking={linking}
            onPick={edgeHandlers.pick}
            register={edgeHandlers.register}
          />
          {selected.size > 0 && (
            <CanvasGroups groups={groupsIn(doc, selected)} boxOf={boxOfId} register={registerGroup} />
          )}
          {doc.cards.map((card) => (
            <Card
              key={card.id}
              card={card}
              selected={selected.has(card.id)}
              editing={editing === card.id}
              live={live === card.id}
              paused={paused(card)}
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
        {doc.cards.length === 0 && doc.frames.length === 0 && !draft && (
          <p className="canvas-hint">Double-click anywhere to add a card</p>
        )}
        <button
          ref={zoomRef}
          type="button"
          className="canvas-zoom"
          title="Back to 100%"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={() => zoomBy(1 / view.current.k)}
        />
      </div>
      {health.safe && (
        <p className="canvas-notice canvas-safe" role="status">
          HTML cards are paused: this canvas stopped responding last time.
          <button type="button" onClick={health.runAnyway}>
            Run cards
          </button>
        </p>
      )}
      {menu && (
        <CanvasMenu
          x={menu.x}
          y={menu.y}
          items={canvasMenuItems(
            menu.at,
            {
              group: canGroup(doc, selected),
              ungroup: canUngroup(doc, selected),
              frame: selected.size > 0,
              remove: selected.size > 0 || picked !== null,
            },
            commands,
            menu.html,
          )}
          onClose={closeMenu}
        />
      )}
    </>
  )
}
