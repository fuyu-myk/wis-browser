import { useState } from 'react'

import type { BrowserState, RailSide, Tab } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { SettingsIcon } from '@/lib/icons'

import { BookmarksButton } from '@/components/BookmarksButton'
import { Brand } from '@/components/Brand'
import { PinnedTabs } from '@/components/PinnedTabs'
import { VpnToggle } from '@/components/VpnToggle'
import { WorkspacePill } from '@/components/WorkspacePill'

interface TopTabsRailProps {
  state: BrowserState
  side: RailSide
  onOpenSettings: () => void
}

/**
 * Slim, always-collapsed rail shown when `layout === 'top_tabs'`. Hosts the
 * brand mark, the workspace pill, and the bookmarks/VPN/settings controls.
 * Expands only while the workspace popover is open.
 */
export function TopTabsRail({ state, side, onOpenSettings }: TopTabsRailProps) {
  const [workspacePopoverOpen, setWorkspacePopoverOpen] = useState(false)
  const [pinnedOpen, setPinnedOpen] = useState(false)
  const expanded = workspacePopoverOpen || pinnedOpen
  const activeWorkspace =
    state.workspaces.find((w) => w.id === state.active_workspace_id) ?? null
  const tabs: Tab[] = activeWorkspace
    ? activeWorkspace.tab_order
        .map((id) => state.tabs.find((t) => t.id === id))
        .filter((t): t is Tab => t !== undefined)
    : []
  const activeTabId = activeWorkspace?.active_tab_id ?? null

  return (
    <aside
      className={cx('kb-side', 'kb-rail', `is-${side}`, {
        'is-expanded': expanded,
      })}
    >
      <Brand />
      <WorkspacePill state={state} onOpenChange={setWorkspacePopoverOpen} />
      {activeWorkspace && (
        <PinnedTabs
          variant="rail"
          workspaceId={activeWorkspace.id}
          tabs={tabs}
          pinnedTabIds={activeWorkspace.pinned_tab_ids}
          activeTabId={activeTabId}
          open={pinnedOpen}
          onOpenChange={setPinnedOpen}
        />
      )}
      <div className="kb-rail-spacer" />
      <div className="kb-rail-controls">
        {!activeWorkspace?.ephemeral && <BookmarksButton variant="rail" />}
        <VpnToggle workspace={activeWorkspace} variant="rail" />
        <button
          type="button"
          className="kb-circle-btn kb-rail-settings"
          aria-label="Settings"
          onClick={onOpenSettings}
        >
          <SettingsIcon />
        </button>
      </div>
    </aside>
  )
}
