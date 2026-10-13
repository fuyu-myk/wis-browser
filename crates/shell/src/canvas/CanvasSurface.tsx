import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
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
  edgeGeometry,
  expandToGroups,
  frameAround,
  framesInside,
  groupCards,
  groupsIn,
  MAX_FRAME_SIZE,
  MIN_FRAME_H,
  MIN_FRAME_W,
  moveFrames,
  newCard,
  newFrame,
  removeCard,
  removeCards,
  removeEdge,
  removeFrame,
  renameFrame,
  resizeFrame,
  tidyName,
  toCanvas,
  ungroupCards,
  updateCard,
  zoomAt,
  type Box,
  type CanvasCard,
  type CanvasDoc,
  type CanvasEdge,
  type CanvasFrame,
  type CanvasView,
} from '@/lib/canvas'
import { cx } from '@/lib/cx'

import { Card, CardEditor, type CardHandlers, type Side } from './CanvasCard'
import { CanvasEdges, type Linking } from './CanvasEdges'
import { Frame, type FrameHandlers } from './CanvasFrame'
import { CanvasGroups, GROUP_PAD, type GroupOutline } from './CanvasGroups'
import { canvasMenuItems, CanvasMenu, type CanvasCommands } from './CanvasMenu'
import { follow, transformOf } from './dom'
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
}

type Selection = ReadonlySet<string>
type Picked = { kind: 'edge' | 'frame'; id: string } | null
type Point = { x: number; y: number }
interface Dragged {
  bases: Map<string, CanvasCard>
  frames: Map<string, CanvasFrame>
  edges: CanvasEdge[]
  dx: number
  dy: number
}

const NONE: Selection = new Set()
const viewTransform = ({ x, y, k }: CanvasView) => `translate3d(${x}px,${y}px,0) scale(${k})`
const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n))
const sameSet = (a: Selection, b: Selection) => a.size === b.size && [...a].every((id) => b.has(id))
const typing = (target: EventTarget | null) =>
  target instanceof Element && target.closest('input, textarea') !== null

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
  const frameEls = useRef(new Map<string, HTMLElement>())
  const groupEls = useRef(new Map<string, GroupOutline>())
  const dragged = useRef<Dragged | null>(null)
  const sizing = useRef<{ id: string; w: number; h: number } | null>(null)
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
  const [renaming, setRenaming] = useState<string | null>(null)
  const [draft, setDraft] = useState<CanvasCard | null>(null)
  const [marquee, setMarquee] = useState<Box | null>(null)
  const [linking, setLinking] = useState<Linking | null>(null)
  const [menu, setMenu] = useState<{ x: number; y: number; at: Point } | null>(null)
  const [panning, setPanning] = useState(false)
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
    const sized = s && frameEls.current.get(s.id)
    if (s && sized) {
      sized.style.width = `${s.w}px`
      sized.style.height = `${s.h}px`
    }
    const d = dragged.current
    if (!d) return
    for (const [id, base] of d.bases) {
      const el = cardEls.current.get(id)
      if (el) el.style.transform = transformOf(base.x + d.dx, base.y + d.dy)
    }
    for (const [id, base] of d.frames) {
      const el = frameEls.current.get(id)
      if (el) el.style.transform = transformOf(base.x + d.dx, base.y + d.dy)
    }
    const cards = new Map(docRef.current.cards.map((c) => [c.id, c]))
    const placed = (id: string) => {
      const card = cards.get(id)
      if (!card) return null
      return d.bases.has(id) ? boxOf(card, d.dx, d.dy) : boxOf(card)
    }
    for (const g of groupEls.current.values()) {
      if (!g.members.some((id) => d.bases.has(id))) continue
      const box = frameAround(
        g.members.flatMap((id) => placed(id) ?? []),
        GROUP_PAD,
      )
      g.el.style.transform = transformOf(box.x, box.y)
      g.el.style.width = `${box.w}px`
      g.el.style.height = `${box.h}px`
    }
    for (const edge of d.edges) {
      const a = placed(edge.from)
      const b = placed(edge.to)
      if (!a || !b) continue
      const path = edgeGeometry(a, b).d
      for (const el of edgeEls.current.get(edge.id) ?? []) el.setAttribute('d', path)
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
  },[commands])

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
    setMenu({ x: e.clientX, y: e.clientY, at: canvasPoint(e.clientX, e.clientY) })
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
      } else {
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
      select(expandToGroups(docRef.current, new Set([id])))
      setEditing(id)
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
      const start = { px: e.clientX, py: e.clientY }
      follow(
        (ev) => {
          const k = view.current.k
          sizing.current = {
            id: target.id,
            w: clamp(target.w + (ev.clientX - start.px) / k, MIN_FRAME_W, MAX_FRAME_SIZE),
            h: clamp(target.h + (ev.clientY - start.py) / k, MIN_FRAME_H, MAX_FRAME_SIZE),
          }
          schedule()
        },
        () => {
          const s = sizing.current
          sizing.current = null
          if (s) props.current.onChange(resizeFrame(docRef.current, s.id, s.w, s.h))
        },
      )
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
          )}
          onClose={closeMenu}
        />
      )}
    </>
  )
}
