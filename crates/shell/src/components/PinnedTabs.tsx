import { useEffect } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Tab, TabId, WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { PinIcon } from '@/lib/icons'
import { tabDisplayName } from '@/lib/url'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import { useTabSplit } from '@/hooks/useTabSplit'

import { TabFavicon } from '@/components/TabFavicon'

interface PinnedTabsProps {
  workspaceId: WorkspaceId
  tabs: Tab[]
  /** Pinned tab IDs in pin order. */
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
 * the tab; right-click opens the tab menu (split + pin/unpin).
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
  const setActive = useMutation({
    mutationFn: (id: TabId) => commands.setActiveTab(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  const { openSplitMenu } = useTabSplit({ workspaceId, activeTabId, onMenuOpenChange })

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

  const pinnedTabs = pinnedTabIds
    .map((id) => tabs.find((t) => t.id === id))
    .filter((t): t is Tab => t !== undefined)

  if (pinnedTabs.length === 0) return null

  const activate = (id: TabId) => {
    setActive.mutate(id)
    if (variant === 'rail') onOpenChange?.(false)
  }

  const grid = (
    <div className="kb-pinned-grid" role="list" aria-label="Pinned tabs">
      {pinnedTabs.map((tab) => (
        <button
          key={tab.id}
          type="button"
          role="listitem"
          className={cx('kb-pinned-cell', { active: tab.id === activeTabId })}
          title={tabDisplayName(tab)}
          aria-label={tabDisplayName(tab)}
          onClick={() => activate(tab.id)}
          onContextMenu={(e) => openSplitMenu(e, tab.id)}
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
