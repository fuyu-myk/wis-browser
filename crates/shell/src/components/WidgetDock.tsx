import { useRef, useState } from 'react'
import { createPortal } from 'react-dom'

import type { Widget, WidgetId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { WidgetFavicon } from '@/components/WidgetFavicon'
import { useUpdateSettings } from '@/hooks/useBrowserState'
import { useWidgetPanel } from '@/hooks/useWidgetPanel'
import { widgetCategory, type WidgetCategory } from '@/lib/widgets'

interface WidgetDockProps {
  widgets: Widget[]
  variant?: 'sidebar' | 'rail'
}

/** Pointer movement (px) before a press becomes a reorder drag. */
const DRAG_THRESHOLD = 5

interface DragState {
  id: WidgetId
  category: WidgetCategory
  startX: number
  startY: number
  moved: boolean
}

/**
 * Quick-launch rows of widget favicons: the social row, a divider, then the AI
 * row, both in the user's stored order (`sidebar_widgets` order is display
 * order). Click toggles the side panel (`useWidgetPanel`); press-drag a
 * favicon along its row to reorder, which persists to settings.
 */
export function WidgetDock({ widgets, variant = 'sidebar' }: WidgetDockProps) {
  const { activeWidgetId, toggleWidget, panelRect } = useWidgetPanel()
  const update = useUpdateSettings()
  const axis: 'x' | 'y' = variant === 'rail' ? 'y' : 'x'

  const social = widgets.filter((w) => widgetCategory(w) === 'social')
  const ai = widgets.filter((w) => widgetCategory(w) === 'ai')

  const dragRef = useRef<DragState | null>(null)
  const suppressClickRef = useRef(false)
  const [draggedId, setDraggedId] = useState<WidgetId | null>(null)
  const [preview, setPreview] = useState<{ category: WidgetCategory; order: WidgetId[] } | null>(
    null,
  )

  const rowOf = (category: WidgetCategory): Widget[] => (category === 'social' ? social : ai)

  const orderedRow = (category: WidgetCategory): Widget[] => {
    const row = rowOf(category)
    if (!preview || preview.category !== category) return row
    return preview.order
      .map((id) => row.find((w) => w.id === id))
      .filter((w): w is Widget => w !== undefined)
  }

  const onPointerDown = (e: React.PointerEvent, w: Widget) => {
    if (e.button !== 0) return
    dragRef.current = {
      id: w.id,
      category: widgetCategory(w),
      startX: e.clientX,
      startY: e.clientY,
      moved: false,
    }
    e.currentTarget.setPointerCapture(e.pointerId)
  }

  const onPointerMove = (e: React.PointerEvent) => {
    const drag = dragRef.current
    if (!drag) return
    if (!drag.moved) {
      const dist = Math.hypot(e.clientX - drag.startX, e.clientY - drag.startY)
      if (dist < DRAG_THRESHOLD) return
      drag.moved = true
      setDraggedId(drag.id)
    }

    // Insertion index from the row's other favicons: count midpoints passed.
    const row = e.currentTarget.closest('.kb-widget-row')
    if (!row) return
    const pointer = axis === 'x' ? e.clientX : e.clientY
    const others = Array.from(row.querySelectorAll<HTMLElement>('[data-wid]')).filter(
      (el) => el.dataset.wid !== drag.id,
    )
    let index = 0
    for (const el of others) {
      const r = el.getBoundingClientRect()
      const mid = axis === 'x' ? r.left + r.width / 2 : r.top + r.height / 2
      if (pointer > mid) index += 1
    }
    const ids = orderedRowIds(rowOf(drag.category), drag.id, index)
    setPreview({ category: drag.category, order: ids })
  }

  const onPointerUp = () => {
    const drag = dragRef.current
    dragRef.current = null
    setDraggedId(null)
    if (!drag) return
    if (drag.moved) {
      suppressClickRef.current = true
      if (preview && preview.category === drag.category) {
        const reordered = orderedRow(drag.category)
        const next =
          drag.category === 'social' ? [...reordered, ...ai] : [...social, ...reordered]
        update.mutate({ sidebar_widgets: next })
      }
    }
    setPreview(null)
  }

  const onClick = (w: Widget) => {
    if (suppressClickRef.current) {
      suppressClickRef.current = false
      return
    }
    toggleWidget(w)
  }

  if (widgets.length === 0) return null

  const renderRow = (category: WidgetCategory) => (
    <div className="kb-widget-row">
      {orderedRow(category).map((w) => (
        <button
          key={w.id}
          type="button"
          data-wid={w.id}
          className={cx('kb-widget-btn', {
            'is-active': w.id === activeWidgetId,
            'is-dragging': w.id === draggedId,
          })}
          title={w.label}
          aria-label={`Open ${w.label}`}
          aria-pressed={w.id === activeWidgetId}
          onPointerDown={(e) => onPointerDown(e, w)}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onClick={() => onClick(w)}
        >
          <WidgetFavicon url={w.url} label={w.label} />
        </button>
      ))}
    </div>
  )

  return (
    <>
      <div
        className={cx('kb-widget-dock', `is-${variant}`)}
        role="toolbar"
        aria-label="Quick launch widgets"
      >
        {social.length > 0 && renderRow('social')}
        {social.length > 0 && ai.length > 0 && <span className="kb-widget-divider" aria-hidden />}
        {ai.length > 0 && renderRow('ai')}
      </div>
      {panelRect !== null &&
        createPortal(
          <div
            className="kb-wpanel-backdrop"
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

/** Row ids with `dragged` moved to `index` among the remaining favicons. */
function orderedRowIds(row: Widget[], dragged: WidgetId, index: number): WidgetId[] {
  const rest = row.map((w) => w.id).filter((id) => id !== dragged)
  const clamped = Math.min(Math.max(index, 0), rest.length)
  return [...rest.slice(0, clamped), dragged, ...rest.slice(clamped)]
}
