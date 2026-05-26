import { useState } from 'react'

import type { BrowserState, RailSide } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { SettingsIcon } from '@/lib/icons'

import { Brand } from '@/components/Brand'
import { WorkspacePill } from '@/components/WorkspacePill'

interface TopTabsRailProps {
  state: BrowserState
  side: RailSide
  onOpenSettings: () => void
}

/**
 * Slim, always-collapsed rail shown when `layout === 'top_tabs'`. Hosts
 * the brand mark, the workspace pill, and the settings button. Only
 * expands while the workspace popover is open.
 */
export function TopTabsRail({ state, side, onOpenSettings }: TopTabsRailProps) {
  const [workspacePopoverOpen, setWorkspacePopoverOpen] = useState(false)
  const expanded = workspacePopoverOpen

  return (
    <aside
      className={cx('kb-side', 'kb-rail', `is-${side}`, {
        'is-expanded': expanded,
      })}
    >
      <Brand />
      <WorkspacePill state={state} onOpenChange={setWorkspacePopoverOpen} />
      <div className="kb-rail-spacer" />
      <button
        type="button"
        className="kb-circle-btn kb-rail-settings"
        aria-label="Settings"
        onClick={onOpenSettings}
      >
        <SettingsIcon />
      </button>
    </aside>
  )
}
