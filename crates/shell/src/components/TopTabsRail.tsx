import { useState } from 'react'

import type { BrowserState, RailSide } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { HistoryIcon, SettingsIcon } from '@/lib/icons'

import { BookmarksButton } from '@/components/BookmarksButton'
import { Brand } from '@/components/Brand'
import { VpnToggle } from '@/components/VpnToggle'
import { WidgetDock } from '@/components/WidgetDock'
import { WorkspacePill } from '@/components/WorkspacePill'

interface TopTabsRailProps {
  state: BrowserState
  side: RailSide
  onOpenSettings: () => void
  onOpenHistory: () => void
}

/**
 * Slim, always-collapsed rail shown when `layout === 'top_tabs'`. Hosts the
 * brand mark, the workspace pill, and the bookmarks/VPN/settings controls.
 * Pinned tabs live in the top strip in this layout. Expands only while the
 * workspace popover is open.
 */
export function TopTabsRail({ state, side, onOpenSettings, onOpenHistory }: TopTabsRailProps) {
  const [workspacePopoverOpen, setWorkspacePopoverOpen] = useState(false)
  const expanded = workspacePopoverOpen
  const activeWorkspace =
    state.workspaces.find((w) => w.id === state.active_workspace_id) ?? null

  return (
    <aside
      className={cx('kb-side', 'kb-rail', `is-${side}`, {
        'is-expanded': expanded,
      })}
    >
      <Brand />
      <WorkspacePill state={state} onOpenChange={setWorkspacePopoverOpen} />
      <div className="kb-rail-spacer" />
      <WidgetDock widgets={state.settings.sidebar_widgets} variant="rail" />
      <div className="kb-rail-controls">
        {!activeWorkspace?.ephemeral && <BookmarksButton variant="rail" />}
        <VpnToggle workspace={activeWorkspace} variant="rail" />
        <button
          type="button"
          className="kb-circle-btn kb-rail-settings"
          aria-label="History"
          onClick={onOpenHistory}
        >
          <HistoryIcon />
        </button>
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
