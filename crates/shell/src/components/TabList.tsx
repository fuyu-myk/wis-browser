import { useRef, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { TabId, WorkspaceId, Tab } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { PlusIcon, XIcon } from '@/lib/icons'
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

interface TabListProps {
  workspaceId: WorkspaceId
  tabs: Tab[]
  activeTabId: TabId | null
  /** Pinned tab IDs in pin order; rendered as a favicon grid above the list. */
  pinnedTabIds: TabId[]
  /** Notified when the right-click split menu opens/closes (expand sidebar). */
  onSplitMenuOpenChange?: (open: boolean) => void
}

/**
 * Vertical tab list shown inside the sidebar. The `TABS` group header is
 * followed by a `+ New tab` row, then the actual tab entries. Each tab
 * renders a coloured "fav" tile (favicon placeholder), title, and a
 * close affordance on hover.
 */
export function TabList({
  workspaceId,
  tabs,
  activeTabId,
  pinnedTabIds,
  onSplitMenuOpenChange,
}: TabListProps) {
  const qc = useQueryClient()
  const setActive = useMutation({
    mutationFn: (id: TabId) => commands.setActiveTab(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  const close = useMutation({
    mutationFn: (id: TabId) => commands.closeTab(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  const createTab = useMutation({
    mutationFn: () => commands.createTab(workspaceId, 'about:blank'),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  const reorder = useMutation({
    mutationFn: (order: TabId[]) => commands.reorderTabs(workspaceId, order),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  const { startTabDrag, draggedRef, openSplitMenu } = useTabSplit({
    workspaceId,
    activeTabId,
    onMenuOpenChange: onSplitMenuOpenChange,
  })

  const discardedTabs = useDiscardedTabs()
  const listRef = useRef<HTMLDivElement>(null)
  const [dropTarget, setDropTarget] = useState<DropTarget | null>(null)

  const pinnedSet = new Set(pinnedTabIds)
  const normalTabs = tabs.filter((t) => !pinnedSet.has(t.id))

  const makeReorder = (tabId: TabId): TabReorder => ({
    targetAtPoint: (x, y) =>
      listRef.current ? tabDropTarget(listRef.current, 'y', x, y) : null,
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

  return (
    <div className="kb-tabs" ref={listRef}>
      <div className="group-label">tabs</div>
      <button
        type="button"
        className="kb-newtab-row"
        onClick={() => createTab.mutate()}
        disabled={!workspaceId || createTab.isPending}
      >
        <PlusIcon />
        <span>New tab</span>
      </button>
      {normalTabs.map((tab, i) => (
        <div
          key={tab.id}
          data-tab-id={tab.id}
          className={cx('kb-tab', {
            active: tab.id === activeTabId,
            asleep: discardedTabs.has(tab.id) && tab.id !== activeTabId,
            'drop-before': dropTarget?.anchorIndex === i && dropTarget.side === 'before',
            'drop-after': dropTarget?.anchorIndex === i && dropTarget.side === 'after',
          })}
          role="button"
          tabIndex={0}
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
        >
          <TabFavicon tab={tab} />
          <span className="ttl">{tabDisplayName(tab)}</span>
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
      ))}
    </div>
  )
}

export type { Tab }
