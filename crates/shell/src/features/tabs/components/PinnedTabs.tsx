import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Tab, TabId, WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { PinIcon } from '@/lib/icons'
import { tabDisplayName } from '@/lib/url'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import { reorderedAmong, useTabSplit } from '@/features/tabs/hooks/useTabSplit'

import { TabFavicon } from '@/features/tabs/components/TabFavicon'
import { moveFocusByArrow } from '@/lib/arrowNav'
import {
  moveByKey,
  movedOrder,
  movedTo,
  onTabRowKey,
  refocusAfterMove,
  withGroupOrder,
} from '@/features/tabs/lib/tabKeys'
import { announce } from '@/lib/announce'
import { patchTabOrder } from '@/features/tabs/lib/tabMotion'

interface PinnedTabsProps {
  workspaceId: WorkspaceId
  tabs: Tab[]
  /** Pinned tab IDs (the set; order follows `tabs`/tab_order). */
  pinnedTabIds: TabId[]
  activeTabId: TabId | null
  /**
   * `sidebar` swaps a favicon grid (expanded) for a single glyph (collapsed)
   * purely via CSS, riding the sidebar's own expand/collapse. `rail` keeps the
   * slim strip and reveals the grid inline only while `open`, which the rail
   * widens to host.
   */
  variant: 'sidebar' | 'rail'
  /** Sidebar only: expand the collapsed sidebar when the glyph is activated. */
  onExpand?: () => void
  /** Notified when the right-click tab menu opens/closes (keep sidebar open). */
  onMenuOpenChange?: (open: boolean) => void
  /** Rail only: whether the inline grid is revealed. */
  open?: boolean
  /** Rail only: request the reveal state to change. */
  onOpenChange?: (open: boolean) => void
}

/**
 * Pinned tabs as a docked quick-access strip under the workspace pill, in both
 * the collapsible sidebar and the top-tabs rail. Clicking a favicon activates
 * the tab; dragging one reorders the pinned row; right-click opens the tab menu
 * (split + pin/unpin).
 */
export function PinnedTabs({
  workspaceId,
  tabs,
  pinnedTabIds,
  activeTabId,
  variant,
  onExpand,
  onMenuOpenChange,
  open = false,
  onOpenChange,
}: PinnedTabsProps) {
  const qc = useQueryClient()
  const invalidate = () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
  const setActive = useMutation({
    mutationFn: (id: TabId) => commands.setActiveTab(id),
    onSuccess: invalidate,
  })
  const reorder = useMutation({
    mutationFn: (order: TabId[]) => commands.reorderTabs(workspaceId, order),
    onMutate: (order) => patchTabOrder(qc, workspaceId, order),
    onSuccess: invalidate,
  })
  const { openSplitMenu } = useTabSplit({ workspaceId, activeTabId, onMenuOpenChange })

  const gridRef = useRef<HTMLDivElement>(null)
  const draggedRef = useRef(false)
  const [dropIndex, setDropIndex] = useState<number | null>(null)

  // Rail: dismiss the inline grid on outside click or Escape.
  useEffect(() => {
    if (variant !== 'rail' || !open) return
    const onDown = (e: PointerEvent) => {
      const t = e.target as Element | null
      if (t?.closest?.('.kb-pinned')) return
      onOpenChange?.(false)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onOpenChange?.(false)
    }
    document.addEventListener('pointerdown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('pointerdown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [variant, open, onOpenChange])

  const pinnedSet = new Set(pinnedTabIds)
  const pinnedTabs = tabs.filter((t) => pinnedSet.has(t.id))

  if (pinnedTabs.length === 0) return null

  const activate = (id: TabId) => {
    setActive.mutate(id)
    if (variant === 'rail') onOpenChange?.(false)
  }

  // Reorder the pinned subset within the full tab order (persists, and reflects
  // back into pinned membership on reload).
  const startDrag = (e: ReactPointerEvent, tabId: TabId) => {
    if (e.button !== 0) return
    const startX = e.clientX
    const startY = e.clientY
    const sourceEl = e.currentTarget as HTMLElement
    const rect = sourceEl.getBoundingClientRect()
    const grabX = startX - rect.left
    const grabY = startY - rect.top
    const fullOrder = tabs.map((t) => t.id)
    draggedRef.current = false
    let begun = false
    let ghost: HTMLElement | null = null
    let lastIndex: number | null = null

    const insertionAt = (x: number, y: number): number => {
      const root = gridRef.current
      if (!root) return pinnedTabs.length
      const cells = Array.from(root.querySelectorAll<HTMLElement>('[data-pin-id]'))
      for (let i = 0; i < cells.length; i++) {
        const r = cells[i].getBoundingClientRect()
        if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) {
          return x < r.left + r.width / 2 ? i : i + 1
        }
      }
      return cells.length
    }

    const move = (ev: PointerEvent) => {
      if (!begun) {
        if (Math.hypot(ev.clientX - startX, ev.clientY - startY) <= 6) return
        begun = true
        draggedRef.current = true
        document.body.classList.add('is-tab-dragging')
        window.getSelection()?.removeAllRanges()
        ghost = sourceEl.cloneNode(true) as HTMLElement
        ghost.classList.add('kb-drag-ghost')
        ghost.style.width = `${rect.width}px`
        ghost.style.height = `${rect.height}px`
        document.body.appendChild(ghost)
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
      ghost?.remove()
      if (begun) document.body.classList.remove('is-tab-dragging')
      setDropIndex(null)
      if (begun && lastIndex != null) {
        const next = reorderedAmong(fullOrder, pinnedSet, tabId, lastIndex)
        if (next) reorder.mutate(next)
      }
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  const movePin = (id: TabId, delta: -1 | 1) => {
    const next = movedOrder(
      pinnedTabs.map((t) => t.id),
      id,
      delta,
    )
    if (!next) return
    reorder.mutate(withGroupOrder(tabs.map((t) => t.id), next))
    refocusAfterMove(`[data-pin-id="${id}"]`)
    announce(movedTo(next.indexOf(id) + 1, next.length))
  }
  const pinStop =
    activeTabId && pinnedTabs.some((t) => t.id === activeTabId) ? activeTabId : pinnedTabs[0]?.id
  const grid = (
    <div
      className="kb-pinned-grid"
      role="group"
      aria-label="Pinned tabs"
      ref={gridRef}
      data-tab-strip
      onKeyDown={(e) => moveFocusByArrow(e, e.currentTarget, '[data-tab-row]', 'both')}
    >
      {pinnedTabs.map((tab, i) => (
        <button
          key={tab.id}
          type="button"
          data-pin-id={tab.id}
          data-tab-row={tab.id}
          aria-current={tab.id === activeTabId ? 'page' : undefined}
          tabIndex={tab.id === pinStop ? 0 : -1}
          className={cx('kb-pinned-cell', {
            active: tab.id === activeTabId,
            'drop-before': dropIndex === i,
            'drop-after': dropIndex === pinnedTabs.length && i === pinnedTabs.length - 1,
          })}
          title={tabDisplayName(tab)}
          aria-label={tabDisplayName(tab)}
          onPointerDown={(e) => startDrag(e, tab.id)}
          onClick={() => {
            if (draggedRef.current) {
              draggedRef.current = false
              return
            }
            activate(tab.id)
          }}
          onContextMenu={(e) => openSplitMenu(e, tab.id)}
          onKeyDown={(e) => {
            if (moveByKey(e, 'horizontal', (delta) => movePin(tab.id, delta))) return
            onTabRowKey(e, {
              activate: () => activate(tab.id),
              menu: (at) => openSplitMenu(at, tab.id),
            })
          }}
        >
          <TabFavicon tab={tab} />
        </button>
      ))}
    </div>
  )

  const label = `${pinnedTabs.length} pinned tab${pinnedTabs.length === 1 ? '' : 's'}`

  if (variant === 'rail') {
    return (
      <div className="kb-pinned is-rail">
        <button
          type="button"
          className="kb-pinned-glyph"
          aria-label={label}
          aria-expanded={open}
          title="Pinned tabs"
          onClick={() => onOpenChange?.(!open)}
        >
          <PinIcon />
        </button>
        {open && grid}
      </div>
    )
  }

  return (
    <div className="kb-pinned">
      <button
        type="button"
        className="kb-pinned-glyph"
        aria-label={label}
        title="Pinned tabs"
        onClick={() => onExpand?.()}
      >
        <PinIcon />
      </button>
      {grid}
    </div>
  )
}
