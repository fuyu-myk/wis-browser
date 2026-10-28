/**
 * Shared "split with this tab" interactions for both tab strips (sidebar list
 * and the top horizontal strip):
 *   - `startTabDrag`: a pointer drag (not HTML5) that, past a small threshold,
 *     fires `wis-tabdrag-begin` so `TabWebViewHost` shows the pane drop targets.
 *   - `openSplitMenu`: a right-click that opens the directional split pill in
 *     the `overlay-tabmenu` webview (composites above the tab webviews, so it
 *     isn't occluded), positioned at the cursor.
 */

import {
  useEffect,
  useRef,
  useState,
  type MutableRefObject,
  type PointerEvent as ReactPointerEvent,
} from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { TabId, WorkspaceId } from '@/bindings/types'
import { clearAfterClick } from '@/features/tabs/lib/dragFlag'
import type { MenuPoint } from '@/features/tabs/lib/tabKeys'

const DRAG_THRESHOLD_PX = 6
// The tab context menu (split + pin + move-to-folder) self-measures and resizes;
// this is just the initial overlay height before it fits to content.
const MENU_W = 200
const MENU_INITIAL_H = 240
const MARGIN = 8

const OPEN_EVENT = 'wis-tabmenu-open'
const READY_EVENT = 'wis-tabmenu-ready'
const DONE_EVENT = 'wis-tabmenu-done'
const PANE_FOCUS_EVENT = 'wis-pane-focus'

interface UseTabSplitArgs {
  workspaceId: WorkspaceId
  activeTabId: TabId | null
  /** Notified when the split menu opens/closes (e.g. to expand the sidebar). */
  onMenuOpenChange?: (open: boolean) => void
}

/**
 * A reorder drop target: where the dragged tab would land (`index`) and how to
 * draw the indicator.
 */
export interface DropTarget {
  index: number
  anchorIndex: number
  side: 'before' | 'after'
}

/**
 * Strip-specific reorder hooks: while dragging within the strip, `targetAtPoint`
 * resolves the cursor to a drop target (or null when the cursor is outside the
 * strip, so a split takes over), `onTarget` drives the insertion indicator, and
 * `onDrop` commits the new order.
 */
export interface TabReorder {
  targetAtPoint: (x: number, y: number) => DropTarget | null
  onTarget: (target: DropTarget | null) => void
  /** Commit the new order; `ghostRect` is where the drag image landed, for
   * the glide-into-place animation. */
  onDrop: (index: number, ghostRect?: DOMRect) => void
  /** Drag lifecycle: true once the threshold is passed, false on release. */
  onDragChange?: (dragging: boolean) => void
}

interface UseTabSplitReturn {
  /** Begin a pointer-drag of `tabId` (drop on a pane to split, or reorder). */
  startTabDrag: (e: ReactPointerEvent, tabId: TabId, reorder?: TabReorder) => void
  /** True when the last press became a drag: check it to suppress the click. */
  draggedRef: MutableRefObject<boolean>
  /** Open the directional split popup for `tabId` (the active tab included). */
  openSplitMenu: (e: MenuPoint, tabId: TabId) => void
}

/**
 * An element's viewport rect with any in-flight transform removed.
 */
export function layoutRect(el: HTMLElement): DOMRect {
  const r = el.getBoundingClientRect()
  const t = getComputedStyle(el).transform
  if (!t || t === 'none') return r
  const m = new DOMMatrixReadOnly(t)
  return new DOMRect(r.x - m.e, r.y - m.f, r.width, r.height)
}

/**
 * Drop target for a drag at (x,y) over a tab strip container, or null when the
 * cursor is outside it. `axis` is 'y' for the vertical sidebar list, 'x' for the
 * horizontal strip. Tabs are matched by their `data-tab-id` attribute.
 *
 * The indicator is anchored to the tab the cursor is physically over so it
 * tracks the cursor: while over a tab's first half (or the gap before it, which
 * belongs to the previous tab) it marks the previous tab's trailing edge; over
 * the first half proper it marks that tab's leading edge. The insertion `index`
 * is identical either way.
 */
export function tabDropTarget(
  container: HTMLElement,
  axis: 'x' | 'y',
  clientX: number,
  clientY: number,
  selector = '[data-tab-id]',
): DropTarget | null {
  const rect = container.getBoundingClientRect()
  if (clientX < rect.left || clientX > rect.right || clientY < rect.top || clientY > rect.bottom) {
    return null
  }
  const els = Array.from(container.querySelectorAll<HTMLElement>(selector))
  if (els.length === 0) return null
  const pos = axis === 'y' ? clientY : clientX
  for (let i = 0; i < els.length; i++) {
    const r = layoutRect(els[i])
    const start = axis === 'y' ? r.top : r.left
    const mid = axis === 'y' ? r.top + r.height / 2 : r.left + r.width / 2
    if (pos < mid) {
      if (pos < start && i > 0) return { index: i, anchorIndex: i - 1, side: 'after' }
      return { index: i, anchorIndex: i, side: 'before' }
    }
  }
  return { index: els.length, anchorIndex: els.length - 1, side: 'after' }
}

/**
 * Build a floating drag image by cloning the grabbed tab. It's pinned to the
 * cursor (positioned by the caller), ignores pointer events so the drop targets
 * still receive them, and is removed when the drag ends. `.kb-drag-ghost`
 * supplies a self-contained active-top-tab pill look (the source's own scoped
 * styles don't apply once it's detached to `document.body`); the `active` class
 * is dropped so it doesn't also paint the sidebar's active indicator bar.
 */
function makeDragGhost(source: HTMLElement, width: number, height: number): HTMLElement {
  const ghost = source.cloneNode(true) as HTMLElement
  ghost.classList.add('kb-drag-ghost')
  ghost.classList.remove('active')
  ghost.removeAttribute('data-tab-id')
  ghost.style.position = 'fixed'
  ghost.style.zIndex = '9999'
  ghost.style.pointerEvents = 'none'
  ghost.style.margin = '0'
  ghost.style.width = `${width}px`
  ghost.style.height = `${height}px`
  return ghost
}

/** New tab order with `draggedId` moved to `insertion`, or null if unchanged. */
export function reorderedTabIds(
  order: TabId[],
  draggedId: TabId,
  insertion: number,
): TabId[] | null {
  const from = order.indexOf(draggedId)
  if (from < 0) return null
  const without = order.filter((id) => id !== draggedId)
  const insertAt = insertion > from ? insertion - 1 : insertion
  without.splice(insertAt, 0, draggedId)
  if (without.length === order.length && without.every((id, i) => id === order[i])) return null
  return without
}

/**
 * Reorder within the non-pinned subset, then merge back so pinned tabs keep
 * their slots in the full order. `insertion` indexes the rendered (non-pinned)
 * list, since pinned tabs live in the quick-access grid, not the list. Returns
 * null when nothing moves.
 */
export function reorderedWithPinned(
  fullOrder: TabId[],
  pinned: ReadonlySet<TabId>,
  draggedId: TabId,
  insertion: number,
): TabId[] | null {
  const nonPinned = fullOrder.filter((id) => !pinned.has(id))
  const next = reorderedTabIds(nonPinned, draggedId, insertion)
  if (!next) return null
  let i = 0
  return fullOrder.map((id) => (pinned.has(id) ? id : next[i++]))
}

/**
 * Insert a tab from outside the `movable` subset at `insertion` (indexing the
 * rendered subset), keeping every other tab's slot in the full order. The
 * dragged tab's folder assignment is changed separately by the caller.
 * Returns null when the order is unchanged.
 */
export function insertedAmong(
  fullOrder: TabId[],
  movable: ReadonlySet<TabId>,
  draggedId: TabId,
  insertion: number,
): TabId[] | null {
  const subset = fullOrder.filter((id) => movable.has(id) && id !== draggedId)
  const without = fullOrder.filter((id) => id !== draggedId)
  const out = [...without]
  if (subset.length === 0) {
    out.push(draggedId)
  } else {
    const anchor = subset[Math.min(insertion, subset.length - 1)]
    const at = out.indexOf(anchor) + (insertion >= subset.length ? 1 : 0)
    out.splice(at, 0, draggedId)
  }
  if (out.length === fullOrder.length && out.every((id, i) => id === fullOrder[i])) return null
  return out
}

/**
 * Reorder only the `movable` subset (e.g. the loose, non-foldered tabs shown as
 * a flat list), merging back so every other tab keeps its slot in the full
 * order. `insertion` indexes the rendered movable list. Returns null when
 * nothing moves.
 */
export function reorderedAmong(
  fullOrder: TabId[],
  movable: ReadonlySet<TabId>,
  draggedId: TabId,
  insertion: number,
): TabId[] | null {
  const subset = fullOrder.filter((id) => movable.has(id))
  const next = reorderedTabIds(subset, draggedId, insertion)
  if (!next) return null
  let i = 0
  return fullOrder.map((id) => (movable.has(id) ? next[i++] : id))
}

export function useTabSplit({
  workspaceId: _workspaceId,
  activeTabId,
  onMenuOpenChange,
}: UseTabSplitArgs): UseTabSplitReturn {
  const draggedRef = useRef(false)
  const ensuredRef = useRef(false)
  const pendingRef = useRef<{ movedTabId: TabId; x: number; y: number; winH: number } | null>(null)
  const [menuOpen, setMenuOpen] = useState(false)

  useEffect(() => {
    onMenuOpenChange?.(menuOpen)
  }, [menuOpen, onMenuOpenChange])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen(READY_EVENT, () => {
      if (pendingRef.current) {
        void emit(OPEN_EVENT, pendingRef.current)
      }
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen(DONE_EVENT, () => {
      pendingRef.current = null
      setMenuOpen(false)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  // Dismiss on a click elsewhere or Escape.
  useEffect(() => {
    if (!menuOpen) return
    const dismiss = () => {
      pendingRef.current = null
      setMenuOpen(false)
      void commands.hideOverlay('tabmenu').catch(() => undefined)
    }
    const onPointerDown = (e: PointerEvent) => {
      if (e.button === 2 && (e.target as HTMLElement | null)?.closest?.('[data-tab-id]')) {
        return
      }
      dismiss()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') dismiss()
    }
    window.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKey)
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen(PANE_FOCUS_EVENT, () => dismiss()).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
      window.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [menuOpen])

  const startTabDrag = (e: ReactPointerEvent, tabId: TabId, reorder?: TabReorder) => {
    if (e.button !== 0) return
    const startX = e.clientX
    const startY = e.clientY

    // Clone the grabbed tab as the drag image; offset so it stays under the
    // cursor exactly where it was picked up.
    const sourceEl = e.currentTarget as HTMLElement
    const sourceRect = sourceEl.getBoundingClientRect()
    const grabX = startX - sourceRect.left
    const grabY = startY - sourceRect.top
    draggedRef.current = false
    let begun = false
    let ghost: HTMLElement | null = null
    let lastTarget: DropTarget | null = null

    const positionGhost = (clientX: number, clientY: number) => {
      if (!ghost) return
      ghost.style.left = `${clientX - grabX}px`
      ghost.style.top = `${clientY - grabY}px`
    }

    const move = (ev: PointerEvent) => {
      if (!begun) {
        if (Math.hypot(ev.clientX - startX, ev.clientY - startY) <= DRAG_THRESHOLD_PX) return
        begun = true
        draggedRef.current = true
        // Suppress text selection (chrome UI) for the duration of the drag.
        document.body.classList.add('is-tab-dragging')
        window.getSelection()?.removeAllRanges()
        ghost = makeDragGhost(sourceEl, sourceRect.width, sourceRect.height)
        document.body.appendChild(ghost)
        positionGhost(ev.clientX, ev.clientY)
        window.dispatchEvent(new CustomEvent('wis-tabdrag-begin', { detail: tabId }))
        reorder?.onDragChange?.(true)
      }
      positionGhost(ev.clientX, ev.clientY)
      // Over the strip → show a reorder indicator; over the content → null, so
      // the pane drop targets take over (split).
      if (reorder) {
        lastTarget = reorder.targetAtPoint(ev.clientX, ev.clientY)
        reorder.onTarget(lastTarget)
      }
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      const ghostRect = ghost?.getBoundingClientRect()
      ghost?.remove()
      ghost = null
      if (begun) {
        document.body.classList.remove('is-tab-dragging')
        clearAfterClick(draggedRef)
      }
      if (begun && reorder) {
        reorder.onTarget(null)
        reorder.onDragChange?.(false)
        if (lastTarget != null) reorder.onDrop(lastTarget.index, ghostRect)
      }
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  const openSplitMenu = (e: MenuPoint, tabId: TabId) => {
    if (!activeTabId) return
    e.preventDefault()
    const winH = window.innerHeight
    const x = Math.min(Math.max(MARGIN, e.clientX), window.innerWidth - MENU_W - MARGIN)
    const y = Math.max(MARGIN, Math.min(e.clientY, winH - MENU_INITIAL_H - MARGIN))
    const payload = { movedTabId: tabId, x, y: e.clientY, winH }
    pendingRef.current = payload

    const place = async () => {
      if (!ensuredRef.current) {
        await commands.ensureOverlay('tabmenu')
        ensuredRef.current = true
      }
      await commands.setOverlayBounds('tabmenu', x, y, MENU_W, MENU_INITIAL_H)
    }
    void place()
      .catch(() => undefined)
      .then(() => emit(OPEN_EVENT, payload))
    setMenuOpen(true)
  }

  return { startTabDrag, draggedRef, openSplitMenu }
}
