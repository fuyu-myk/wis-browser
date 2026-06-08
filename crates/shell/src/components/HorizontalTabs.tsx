import { useLayoutEffect, useRef, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Tab, TabId, WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { LockIcon, XIcon } from '@/lib/icons'
import { tabDisplayName } from '@/lib/url'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import {
  useTabSplit,
  tabDropTarget,
  reorderedWithPinned,
  type TabReorder,
  type DropTarget,
} from '@/hooks/useTabSplit'

import { TabFavicon } from '@/components/TabFavicon'
import { useDiscardedTabs } from '@/components/DiscardedTabsContext'

interface HorizontalTabsProps {
  workspaceId: WorkspaceId
  tabs: Tab[]
  activeTabId: TabId | null
  /** Pinned tab IDs; pulled out of the strip (they live in the rail). */
  pinnedTabIds: TabId[]
  /** When true, render the URL inside the active tab. */
  integrateUrlBar: boolean
  /** Render as a smaller, centered tab strip. */
  compact?: boolean
}

/* Phase boundary: active tab can't shrink below this (in px). */
const MINIMUM_CURRENT_TAB_SIZE = 260
const COMPACT_MINIMUM_CURRENT_TAB_SIZE = 180
/* Final floor: inactive tabs can't shrink below this (favicon visible). */
const MINIMUM_TAB_SIZE = 44
const COMPACT_MINIMUM_TAB_SIZE = 40
const GAP_PX = 4

interface SizingResult {
  activeSize: number
  inactiveSize: number
  /** True when total content exceeds the strip and scrolling is needed. */
  scrolling: boolean
}

/**
 * Horizontal tab strip.
 * Layout rules:
 *   • One tab fills the row, two tabs each fill half,
 *     n tabs each fill 1/n. Active and inactive same size.
 *   • If 1/n would be smaller than the active tab's
 *     MINIMUM_CURRENT_TAB_SIZE, the active tab pins to that
 *     minimum and the remaining width is split equally across inactives.
 *   • Once the inactive share would be smaller than MINIMUM_TAB_SIZE
 *     (the favicon-only floor), inactives pin to that floor and the strip scrolls.
 *     The active tab is always scrolled into view.
 */
function computeSizing(
  n: number,
  stripWidth: number,
  minActive: number,
  minInactive: number,
): SizingResult {
  if (n <= 0 || stripWidth <= 0) {
    return { activeSize: minActive, inactiveSize: minInactive, scrolling: false }
  }
  const budget = stripWidth - (n - 1) * GAP_PX

  const equalSize = budget / n
  if (equalSize >= minActive) {
    return { activeSize: equalSize, inactiveSize: equalSize, scrolling: false }
  }

  if (n === 1) {
    return { activeSize: minActive, inactiveSize: minInactive, scrolling: false }
  }
  const widthForInactives = budget - minActive
  const inactiveCount = n - 1
  const calcInactiveSize = widthForInactives / inactiveCount

  if (calcInactiveSize >= minInactive) {
    return { activeSize: minActive, inactiveSize: calcInactiveSize, scrolling: false }
  }

  return { activeSize: minActive, inactiveSize: minInactive, scrolling: true }
}

export function HorizontalTabs({
  workspaceId,
  tabs,
  activeTabId,
  pinnedTabIds,
  integrateUrlBar,
  compact = false,
}: HorizontalTabsProps) {
  const qc = useQueryClient()
  const setActive = useMutation({
    mutationFn: (id: TabId) => commands.setActiveTab(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  const close = useMutation({
    mutationFn: (id: TabId) => commands.closeTab(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  const reorder = useMutation({
    mutationFn: (order: TabId[]) => commands.reorderTabs(workspaceId, order),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  const { startTabDrag, draggedRef, openSplitMenu } = useTabSplit({
    workspaceId,
    activeTabId,
  })

  const discardedTabs = useDiscardedTabs()
  const wrapRef = useRef<HTMLDivElement>(null)
  const stripRef = useRef<HTMLDivElement>(null)
  const activeRef = useRef<HTMLDivElement>(null)
  const [stripWidth, setStripWidth] = useState(0)
  const [dropTarget, setDropTarget] = useState<DropTarget | null>(null)

  const pinnedSet = new Set(pinnedTabIds)
  const stripTabs = tabs.filter((t) => !pinnedSet.has(t.id))

  const makeReorder = (tabId: TabId): TabReorder => ({
    targetAtPoint: (x, y) =>
      stripRef.current ? tabDropTarget(stripRef.current, 'x', x, y) : null,
    onTarget: setDropTarget,
    onDrop: (idx) => {
      const next = reorderedWithPinned(
        tabs.map((t) => t.id),
        pinnedSet,
        tabId,
        idx,
      )
      if (next) reorder.mutate(next)
    },
  })

  useLayoutEffect(() => {
    const strip = stripRef.current
    if (!strip) return
    const update = () => setStripWidth(strip.clientWidth)
    update()
    const ro = new ResizeObserver(update)
    ro.observe(strip)
    return () => ro.disconnect()
  }, [])

  // Scroll the active pill into view whenever the active id changes
  useLayoutEffect(() => {
    const el = activeRef.current
    if (!el) return
    el.scrollIntoView({ block: 'nearest', inline: 'center', behavior: 'smooth' })
  }, [activeTabId])

  const n = stripTabs.length
  const minActive = compact ? COMPACT_MINIMUM_CURRENT_TAB_SIZE : MINIMUM_CURRENT_TAB_SIZE
  const minInactive = compact ? COMPACT_MINIMUM_TAB_SIZE : MINIMUM_TAB_SIZE

  if (n === 0) {
    return (
      <div className={cx('kb-tabs-h-wrap', { compact })} ref={wrapRef}>
        <div className={cx('tabs-h', { compact })} ref={stripRef} />
      </div>
    )
  }

  const sizing = computeSizing(n, stripWidth, minActive, minInactive)

  const isMany = sizing.activeSize !== sizing.inactiveSize
  const inactiveCloseThreshold = compact ? 90 : 130
  const inactiveCompact = isMany && sizing.inactiveSize < inactiveCloseThreshold

  return (
    <div
      className={cx('kb-tabs-h-wrap', {
        compact,
        'is-many': isMany,
        'is-scrolling': sizing.scrolling,
        'is-inactive-compact': inactiveCompact,
      })}
      ref={wrapRef}
    >
      <div className={cx('tabs-h', { compact })} ref={stripRef}>
        {stripTabs.map((tab, i) => {
          const isActive = tab.id === activeTabId
          const size = isActive ? sizing.activeSize : sizing.inactiveSize
          return (
            <div
              key={tab.id}
              ref={isActive ? activeRef : undefined}
              data-tab-id={tab.id}
              role="button"
              tabIndex={0}
              className={cx('h-tab', {
                active: isActive,
                asleep: discardedTabs.has(tab.id) && !isActive,
                'as-urlbar': isActive && integrateUrlBar,
                'drop-before': dropTarget?.anchorIndex === i && dropTarget.side === 'before',
                'drop-after': dropTarget?.anchorIndex === i && dropTarget.side === 'after',
              })}
              style={{ flex: `0 0 ${size}px` }}
              onPointerDown={(e) => startTabDrag(e, tab.id, makeReorder(tab.id))}
              onClick={() => {
                if (draggedRef.current) {
                  draggedRef.current = false
                  return
                }
                setActive.mutate(tab.id)
              }}
              onContextMenu={(e) => openSplitMenu(e, tab.id)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault()
                  setActive.mutate(tab.id)
                }
              }}
              title={tab.title}
            >
              {isActive && integrateUrlBar ? (
                <UrlContent tab={tab} />
              ) : (
                <>
                  <TabFavicon tab={tab} />
                  <span className="ttl">{tabDisplayName(tab)}</span>
                </>
              )}
              <button
                type="button"
                className="close"
                aria-label="Close tab"
                onClick={(e) => {
                  e.stopPropagation()
                  close.mutate(tab.id)
                }}
              >
                <XIcon />
              </button>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function UrlContent({ tab }: { tab: Tab }) {
  const parts = (() => {
    try {
      const u = new URL(tab.url)
      if (u.protocol === 'about:' || u.host === '') {
        return null
      }
      return { domain: u.host, path: u.pathname === '/' ? '' : u.pathname }
    } catch {
      return null
    }
  })()

  return (
    <>
      <LockIcon className="ico" />
      {parts ? (
        <>
          <span className="domain">{parts.domain}</span>
          <span className="path">{parts.path}</span>
        </>
      ) : (
        <span className="path">Search or enter URL</span>
      )}
    </>
  )
}
