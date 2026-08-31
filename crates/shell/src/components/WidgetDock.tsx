import { Fragment, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

import type { Widget, WidgetId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { WidgetFavicon } from '@/components/WidgetFavicon'
import { useUpdateSettings } from '@/hooks/useBrowserState'
import { useWidgetPanel } from '@/hooks/useWidgetPanel'
import {
  dockSignature,
  placeWidget,
  WIDGET_GROUPS,
  widgetCategory,
  type WidgetCategory,
} from '@/lib/widgets'

interface WidgetDockProps {
  widgets: Widget[]
  variant?: 'sidebar' | 'rail'
}

/** Pointer movement (px) before a press becomes a reorder drag. */
const DRAG_THRESHOLD = 5

/** How near the dock's edge starts scrolling, and how fast it scrolls. */
const AUTOSCROLL_EDGE = 24
/** Creep speed in px per second. */
const AUTOSCROLL_SPEED = 360
/** The longest frame gap honoured. */
const AUTOSCROLL_MAX_STEP_MS = 50

interface DragState {
  id: WidgetId
  /** The stored row the drag started from. */
  from: WidgetCategory
  /** Customs may be dragged across rows; curated widgets stay in their own row. */
  isCustom: boolean
  startX: number
  startY: number
  moved: boolean
  /** Where this gesture would land, per-gesture. */
  drop: DropPreview | null
  /** True once a target was resolved, even if later cleared. */
  resolved: boolean
}

/** Where the drop will land. */
interface DropPreview {
  target: WidgetCategory
  index: number
  /** The target row's flow, for the marker's orientation. */
  horizontal: boolean
}

/**
 * Which slot the drop caret marks in `row`, given an `index` counted among
 * the row's widgets excluding the dragged one.
 */
function caretSlot(row: readonly Widget[], draggedId: WidgetId, index: number): number {
  let counted = 0
  for (let pos = 0; pos < row.length; pos += 1) {
    if (counted === index) return pos
    if (row[pos].id !== draggedId) counted += 1
  }
  return row.length
}

/** Vertical gap between `clientY` and a span, zero when inside it. */
function verticalDistance(top: number, bottom: number, clientY: number): number {
  if (clientY < top) return top - clientY
  if (clientY > bottom) return clientY - bottom
  return 0
}

/** The row `clientY` is in, or the nearest one. */
function nearestRow(
  rows: readonly HTMLElement[],
  clientY: number,
  port: DOMRect,
): HTMLElement | null {
  let best: HTMLElement | null = null
  let bestDistance = Infinity
  for (const row of rows) {
    const rect = row.getBoundingClientRect()
    if (rect.bottom <= port.top || rect.top >= port.bottom) continue
    const distance = verticalDistance(rect.top, rect.bottom, clientY)
    if (distance < bestDistance) {
      bestDistance = distance
      best = row
    }
  }
  return best
}

interface RowLine {
  top: number
  bottom: number
  mids: Array<{ x: number; y: number }>
}

/**
 * A row's favicons bucketed into visual lines by vertical overlap, ignoring
 * `exceptId`.
 */
function rowLines(row: HTMLElement, exceptId: WidgetId): RowLine[] {
  const lines: RowLine[] = []
  for (const el of row.querySelectorAll<HTMLElement>('[data-wid]')) {
    if (el.dataset.wid === exceptId) continue
    const r = el.getBoundingClientRect()
    const mid = { x: r.left + r.width / 2, y: r.top + r.height / 2 }
    const line = lines.find((l) => r.top < l.bottom - 1 && r.bottom > l.top + 1)
    if (line) {
      line.mids.push(mid)
      line.top = Math.min(line.top, r.top)
      line.bottom = Math.max(line.bottom, r.bottom)
    } else {
      lines.push({ top: r.top, bottom: r.bottom, mids: [mid] })
    }
  }
  return lines
}

/**
 * Quick-launch rows of widget favicons, one row per category in
 * `WIDGET_GROUPS` order with dividers between rows, each row in the user's
 * stored order (`sidebar_widgets` order is display order). Click toggles the
 * side panel (`useWidgetPanel`); press-drag a favicon along its row to
 * reorder. A custom widget can also be dragged onto another row, which stores
 * that row as its group. Curated widgets never leave their own row.
 */
export function WidgetDock({ widgets, variant = 'sidebar' }: WidgetDockProps) {
  const { activeWidgetId, toggleWidget, panelRect, panelSide } = useWidgetPanel()
  const update = useUpdateSettings()

  const byCategory = (category: WidgetCategory): Widget[] =>
    widgets.filter((w) => widgetCategory(w) === category)

  const dockRef = useRef<HTMLDivElement | null>(null)
  const dragRef = useRef<DragState | null>(null)
  const suppressClickRef = useRef(false)
  /** Detaches the window listeners of the gesture in flight. */
  const detachRef = useRef<(() => void) | null>(null)

  const [dragged, setDragged] = useState<{
    id: WidgetId
    from: WidgetCategory
    isCustom: boolean
  } | null>(null)
  const [preview, setPreview] = useState<DropPreview | null>(null)
  /** Which edges have content past them, for the dock's cut-off fade. */
  const [edges, setEdges] = useState({ up: false, down: false })

  const widgetsRef = useRef(widgets)
  widgetsRef.current = widgets
  /** Last pointer position. */
  const lastPointRef = useRef<{ x: number; y: number } | null>(null)
  /** -1 up, 1 down, 0 still: the dock's edge-scroll while dragging. */
  const scrollDirRef = useRef(0)
  const scrollRafRef = useRef<number | null>(null)
  const scrollClockRef = useRef(0)

  const stopAutoScroll = () => {
    scrollDirRef.current = 0
    scrollClockRef.current = 0
    if (scrollRafRef.current !== null) {
      cancelAnimationFrame(scrollRafRef.current)
      scrollRafRef.current = null
    }
  }

  /**
   * Creep the dock while the pointer rests near an edge.
   */
  const stepAutoScroll = (now: number) => {
    scrollRafRef.current = null
    const dock = dockRef.current
    const point = lastPointRef.current
    if (dragRef.current === null || dock === null || point === null) return
    if (scrollDirRef.current === 0) return

    const last = scrollClockRef.current
    scrollClockRef.current = now
    const elapsed = last === 0 ? 16 : Math.min(now - last, AUTOSCROLL_MAX_STEP_MS)
    const before = dock.scrollTop
    dock.scrollTop = before + (scrollDirRef.current * AUTOSCROLL_SPEED * elapsed) / 1000
    if (dock.scrollTop === before) return
    // Re-arm before re-resolving.
    scrollRafRef.current = requestAnimationFrame(stepAutoScroll)
    trackRef.current(point.x, point.y)
  }

  useEffect(
    () => () => {
      detachRef.current?.()
      if (scrollRafRef.current !== null) cancelAnimationFrame(scrollRafRef.current)
    },
    [],
  )

  useEffect(() => {
    const dock = dockRef.current
    if (dock === null) return
    const measure = () => {
      const slack = dock.scrollHeight - dock.clientHeight
      const up = dock.scrollTop > 1
      const down = slack > 1 && dock.scrollTop < slack - 1
      setEdges((prev) => (prev.up === up && prev.down === down ? prev : { up, down }))
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(dock)
    dock.addEventListener('scroll', measure, { passive: true })
    return () => {
      observer.disconnect()
      dock.removeEventListener('scroll', measure)
    }
  }, [widgets, dragged])

  useEffect(() => {
    const dock = dockRef.current
    if (dragged === null || dock === null) return
    const reresolve = () => {
      const point = lastPointRef.current
      if (point !== null) trackRef.current(point.x, point.y)
    }
    const observer = new ResizeObserver(reresolve)
    observer.observe(dock)
    dock.addEventListener('scroll', reresolve, { passive: true })
    return () => {
      observer.disconnect()
      dock.removeEventListener('scroll', reresolve)
    }
  }, [dragged])

  const trackPointer = (clientX: number, clientY: number) => {
    const drag = dragRef.current
    const dock = dockRef.current
    if (!drag || !dock) return
    lastPointRef.current = { x: clientX, y: clientY }
    if (!drag.moved) {
      if (Math.hypot(clientX - drag.startX, clientY - drag.startY) < DRAG_THRESHOLD) return
      drag.moved = true
      setDragged({ id: drag.id, from: drag.from, isCustom: drag.isCustom })
      return
    }

    const rows = Array.from(dock.querySelectorAll<HTMLElement>('.kb-widget-row'))
    const home = rows.find((r) => r.dataset.category === drag.from) ?? null
    // Everything resolves against the dock's visible port.
    const port = dock.getBoundingClientRect()
    const y = Math.min(Math.max(clientY, port.top + 1), port.bottom - 1)

    // A curated widget is pinned to its own row, so creeping past that row
    // only pulls the caret out of sight; a custom may reach any row.
    const bound = drag.isCustom ? null : (home?.getBoundingClientRect() ?? null)
    const upward = dock.scrollTop > 0 && (bound === null || bound.top < port.top)
    const downward =
      dock.scrollTop + dock.clientHeight < dock.scrollHeight &&
      (bound === null || bound.bottom > port.bottom)
    const edge = Math.min(AUTOSCROLL_EDGE, port.height / 3)
    const direction =
      clientY < port.top + edge && upward
        ? -1
        : clientY > port.bottom - edge && downward
          ? 1
          : 0
    scrollDirRef.current = direction
    if (direction !== 0 && scrollRafRef.current === null) {
      scrollClockRef.current = 0
      scrollRafRef.current = requestAnimationFrame(stepAutoScroll)
    }
    const row = drag.isCustom ? (nearestRow(rows, y, port) ?? home) : home
    if (!row) return
    const rowRect = row.getBoundingClientRect()
    if (rowRect.bottom <= port.top || rowRect.top >= port.bottom) {
      if (drag.drop !== null) {
        drag.drop = null
        setPreview(null)
      }
      return
    }
    const target = (row.dataset.category as WidgetCategory | undefined) ?? drag.from

    // Insertion index from the row's real geometry.
    const lines = rowLines(row, drag.id)
    const horizontal = lines.some((l) => l.mids.length > 1)
      ? true
      : lines.length > 1
        ? false
        : rowRect.width > rowRect.height

    let index = 0
    let chosen: RowLine | null = null
    let chosenBefore = 0
    let bestDistance = Infinity
    let before = 0
    for (const line of lines) {
      const distance = verticalDistance(line.top, line.bottom, y)
      if (distance < bestDistance) {
        bestDistance = distance
        chosen = line
        chosenBefore = before
      }
      before += line.mids.length
    }
    if (chosen !== null) {
      const pointer = horizontal ? clientX : y
      const passed = chosen.mids.filter((m) => pointer > (horizontal ? m.x : m.y)).length
      index = chosenBefore + passed
    }
    drag.resolved = true
    const previous = drag.drop
    if (
      previous === null ||
      previous.target !== target ||
      previous.index !== index ||
      previous.horizontal !== horizontal
    ) {
      const drop: DropPreview = { target, index, horizontal }
      drag.drop = drop
      setPreview(drop)
    }
  }

  const trackRef = useRef(trackPointer)
  trackRef.current = trackPointer

  /** End the gesture, committing the standing preview only for a real release. */
  const settle = (commit: boolean) => {
    const drag = dragRef.current
    detachRef.current?.()
    stopAutoScroll()
    dragRef.current = null
    lastPointRef.current = null
    if (drag !== null && drag.moved && drag.resolved) {
      suppressClickRef.current = true
      const current = widgetsRef.current
      const next =
        commit && drag.drop !== null
          ? placeWidget(current, drag.id, drag.drop.target, drag.drop.index)
          : null
      if (next !== null && dockSignature(next) !== dockSignature(current)) {
        update.mutate({ sidebar_widgets: next })
      }
    }
    setDragged(null)
    setPreview(null)
  }

  const onPointerDown = (e: React.PointerEvent, w: Widget) => {
    if (e.button !== 0) return
    if (dragRef.current !== null) settle(false)
    suppressClickRef.current = false
    dragRef.current = {
      id: w.id,
      from: widgetCategory(w),
      isCustom: w.kind === 'custom',
      startX: e.clientX,
      startY: e.clientY,
      moved: false,
      drop: null,
      resolved: false,
    }

    const pointerId = e.pointerId
    const mine = (ev: PointerEvent) => ev.pointerId === pointerId
    const onMove = (ev: PointerEvent) => {
      if (!mine(ev)) return
      if (ev.buttons === 0) settle(false)
      else trackPointer(ev.clientX, ev.clientY)
    }
    const onUp = (ev: PointerEvent) => {
      if (mine(ev) && ev.button === 0) settle(true)
    }
    const onCancel = (ev: PointerEvent) => {
      if (mine(ev)) settle(false)
    }
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === 'Escape') settle(false)
    }
    const onBlur = () => settle(false)
    detachRef.current?.()
    detachRef.current = () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      window.removeEventListener('pointercancel', onCancel)
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('blur', onBlur)
      detachRef.current = null
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
    window.addEventListener('pointercancel', onCancel)
    window.addEventListener('keydown', onKey)
    window.addEventListener('blur', onBlur)
  }

  const onClick = (e: React.MouseEvent, w: Widget) => {
    // detail 0 is a keyboard activation, which no drag precedes.
    if (suppressClickRef.current && e.detail !== 0) {
      suppressClickRef.current = false
      return
    }
    toggleWidget(w)
  }

  if (widgets.length === 0) return null

  // Only a custom can change rows, so only its drag opens the empty placeholders.
  const dragActive = dragged?.isCustom === true

  const renderButton = (w: Widget, caret: 'before' | 'after' | null) => (
    <button
      key={w.id}
      type="button"
      data-wid={w.id}
      className={cx('kb-widget-btn', {
        'is-active': w.id === activeWidgetId,
        'is-dragging': w.id === dragged?.id,
        'is-drop-before': caret === 'before',
        'is-drop-after': caret === 'after',
      })}
      title={w.label}
      aria-label={`Open ${w.label}`}
      aria-pressed={w.id === activeWidgetId}
      onPointerDown={(e) => onPointerDown(e, w)}
      onClick={(e) => onClick(e, w)}
    >
      <WidgetFavicon url={w.url} label={w.label} />
    </button>
  )

  const renderRow = (category: WidgetCategory) => {
    const base = byCategory(category)
    const targeted = dragged !== null && preview !== null && preview.target === category
    const at =
      targeted && dragged !== null && preview !== null
        ? caretSlot(base, dragged.id, preview.index)
        : -1

    return (
      <div
        className={cx('kb-widget-row', {
          'is-stacked': targeted && preview !== null && !preview.horizontal,
          'is-slot': base.length === 0 && dragActive,
          'is-target': base.length === 0 && targeted,
        })}
        data-category={category}
      >
        {base.map((w, i) =>
          renderButton(
            w,
            at === i ? 'before' : at === base.length && i === base.length - 1 ? 'after' : null,
          ),
        )}
      </div>
    )
  }

  const visibleGroups = dragActive
    ? WIDGET_GROUPS
    : WIDGET_GROUPS.filter((category) => byCategory(category).length > 0)

  return (
    <>
      <div
        ref={dockRef}
        className={cx('kb-widget-dock', `is-${variant}`, {
          'can-scroll-up': edges.up && dragged === null,
          'can-scroll-down': edges.down && dragged === null,
        })}
        role="toolbar"
        aria-label="Quick launch widgets"
      >
        {visibleGroups.map((category, i) => (
          <Fragment key={category}>
            <span className={cx('kb-widget-divider', { 'is-hidden': i === 0 })} aria-hidden />
            {renderRow(category)}
          </Fragment>
        ))}
      </div>
      {panelRect !== null &&
        createPortal(
          <div
            key={activeWidgetId ?? 'none'}
            className={cx('kb-wpanel-backdrop', { 'is-right': panelSide === 'right' })}
            style={{
              left: panelRect.x,
              top: panelRect.y,
              width: panelRect.width,
              height: panelRect.height,
            }}
            aria-hidden
          />,
          document.body,
        )}
    </>
  )
}
