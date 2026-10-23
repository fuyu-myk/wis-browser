import { useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'

import { partOffset, runPendingGlide, type PendingGlide } from '@/features/tabs/lib/tabMotion'

/** Pointer movement (px) before a press becomes a reorder drag. */
const DRAG_THRESHOLD = 5

export interface PointerReorderOptions {
  /** Stable item keys in current display order. */
  keys: string[]
  /** Axis the items flow along (`x` for a row, `y` for a column). */
  axis: 'x' | 'y'
  /** Selector for the element wrapping the draggable items; siblings are found
   *  within it by their `data-key` attribute, and the dropped item glides home
   *  inside it. */
  containerSelector: string
  /** Extra class on the floating ghost clone, so each surface can style it. */
  ghostClass?: string
  /** Commit a new key order. */
  onReorder: (orderedKeys: string[]) => void
}

export interface PointerReorder {
  /** Key being dragged (its source goes invisible; the ghost is the moving
   *  copy), or null when idle. */
  draggedKey: string | null
  /** Parting transform for the item at original `index` (siblings make room),
   *  or undefined. Spread onto the item's inline style. */
  itemTransform: (index: number) => string | undefined
  /** Pointer handler to spread on each item's drag surface (the whole item or a
   *  dedicated grip). The item element must carry `data-key={key}`. */
  dragHandlers: (key: string) => { onPointerDown: (e: ReactPointerEvent) => void }
  /** True (once) immediately after a drag, so a click-to-act item can ignore
   *  the trailing click a press-drag would otherwise fire. */
  consumeClickSuppressed: () => boolean
}

/**
 * Pointer-based "pick up and hold-drag" reordering with the tab-strip
 * choreography: a clone of the grabbed item floats under the cursor
 * (`kb-drag-ghost`), siblings part via `partOffset` so a gap travels to the
 * target, and on release the dropped item glides from the ghost into its
 * committed slot (`glideIntoPlace`). The grabbed item keeps its slot (so parted
 * siblings pre-occupy their final positions and never jump on commit).
 */
export function usePointerReorder(options: PointerReorderOptions): PointerReorder {
  const optionsRef = useRef(options)
  optionsRef.current = options
  const draggedFlag = useRef(false)
  const slotRef = useRef(0)
  const pendingGlideRef = useRef<PendingGlide | null>(null)
  const [draggedKey, setDraggedKey] = useState<string | null>(null)
  const [dropIndex, setDropIndex] = useState<number | null>(null)

  // After the commit re-render, glide the dropped item from the ghost's last
  // position into its new slot. No-ops when nothing is pending.
  useLayoutEffect(() => {
    const root = document.querySelector<HTMLElement>(optionsRef.current.containerSelector)
    runPendingGlide(root, pendingGlideRef)
  })

  const startDrag = (e: ReactPointerEvent, key: string) => {
    if (e.button !== 0) return
    const itemEl = (e.currentTarget as HTMLElement).closest<HTMLElement>('[data-key]')
    if (!itemEl) return
    const startX = e.clientX
    const startY = e.clientY
    const rect = itemEl.getBoundingClientRect()
    const grabX = startX - rect.left
    const grabY = startY - rect.top
    const { axis, containerSelector, ghostClass } = optionsRef.current
    draggedFlag.current = false
    let begun = false
    let ghost: HTMLElement | null = null
    let mids: number[] = []
    let lastIndex: number | null = null

    // Insertion slot: sibling midpoints the pointer has passed. Measured against
    // a start-of-drag snapshot so parting transforms don't move the targets.
    const insertionAt = (x: number, y: number): number => {
      const pointer = axis === 'x' ? x : y
      let idx = 0
      for (const mid of mids) if (pointer > mid) idx += 1
      return idx
    }

    const move = (ev: PointerEvent) => {
      if (!begun) {
        if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < DRAG_THRESHOLD) return
        begun = true
        draggedFlag.current = true
        const container = itemEl.closest(containerSelector)
        const cells = container
          ? Array.from(container.querySelectorAll<HTMLElement>('[data-key]'))
          : [itemEl]
        const center = (el: HTMLElement) => {
          const r = el.getBoundingClientRect()
          return axis === 'x' ? r.left + r.width / 2 : r.top + r.height / 2
        }
        mids = cells.map(center)
        slotRef.current = cells.length > 1 ? Math.abs(mids[1] - mids[0]) : axis === 'x' ? rect.width : rect.height
        document.body.classList.add('is-tab-dragging')
        window.getSelection()?.removeAllRanges()
        ghost = itemEl.cloneNode(true) as HTMLElement
        ghost.classList.add('kb-drag-ghost')
        if (ghostClass) ghost.classList.add(ghostClass)
        ghost.style.width = `${rect.width}px`
        ghost.style.height = `${rect.height}px`
        document.body.appendChild(ghost)
        setDraggedKey(key)
      }
      if (ghost) {
        ghost.style.left = `${ev.clientX - grabX}px`
        ghost.style.top = `${ev.clientY - grabY}px`
      }
      lastIndex = insertionAt(ev.clientX, ev.clientY)
      setDropIndex(lastIndex)
    }

    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      const ghostRect = ghost?.getBoundingClientRect()
      ghost?.remove()
      if (begun) document.body.classList.remove('is-tab-dragging')
      setDraggedKey(null)
      setDropIndex(null)
      if (begun && lastIndex != null) {
        const order = reorderToIndex(optionsRef.current.keys, key, lastIndex)
        if (order) {
          if (ghostRect) {
            const sel = `[data-key="${cssEscape(key)}"]`
            pendingGlideRef.current = { selector: sel, rect: ghostRect }
          }
          optionsRef.current.onReorder(order)
        }
      }
    }

    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  const itemTransform = (index: number): string | undefined => {
    if (draggedKey == null || dropIndex == null) return undefined
    const from = optionsRef.current.keys.indexOf(draggedKey)
    if (from < 0) return undefined
    const shift = partOffset(index, from, dropIndex)
    if (shift === 0) return undefined
    const px = shift * slotRef.current
    return optionsRef.current.axis === 'x' ? `translateX(${px}px)` : `translateY(${px}px)`
  }

  return {
    draggedKey,
    itemTransform,
    dragHandlers: (key) => ({ onPointerDown: (e) => startDrag(e, key) }),
    consumeClickSuppressed: () => {
      if (draggedFlag.current) {
        draggedFlag.current = false
        return true
      }
      return false
    },
  }
}

/**
 * `keys` with `key` moved so it lands before the item currently at slot `to`
 * (`to === keys.length` means last). Returns null when the order is unchanged.
 */
function reorderToIndex(keys: string[], key: string, to: number): string[] | null {
  const from = keys.indexOf(key)
  if (from < 0) return null
  const rest = keys.filter((k) => k !== key)
  // Removing the item shifts everything after it down one slot.
  const insertAt = from < to ? to - 1 : to
  const clamped = Math.min(Math.max(insertAt, 0), rest.length)
  const next = [...rest.slice(0, clamped), key, ...rest.slice(clamped)]
  if (next.length === keys.length && next.every((k, i) => k === keys[i])) return null
  return next
}

/** Minimal CSS.escape fallback for the attribute selector (keys are URLs). */
function cssEscape(value: string): string {
  const escaper = (globalThis as { CSS?: { escape?: (v: string) => string } }).CSS?.escape
  return escaper ? escaper(value) : value.replace(/["\\]/g, '\\$&')
}
