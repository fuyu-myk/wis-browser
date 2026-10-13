import { useEffect, useRef, useState } from 'react'

import type { BrowserState, RailSide } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { AssistantIcon, CanvasIcon, HistoryIcon, SettingsIcon, UsageIcon } from '@/lib/icons'

import { BookmarksButton } from '@/components/BookmarksButton'
import { Brand } from '@/components/Brand'
import { DownloadsButton } from '@/components/DownloadsButton'
import { VpnToggle } from '@/components/VpnToggle'
import { WidgetDock } from '@/components/WidgetDock'
import { WorkspacePill } from '@/components/WorkspacePill'

interface TopTabsRailProps {
  state: BrowserState
  side: RailSide
  onOpenSettings: () => void
  onOpenHistory: () => void
  onOpenCanvases: () => void
  /** False until a provider key is stored, which hides the assistant entirely. */
  aiAvailable: boolean
  aiOpen: boolean
  onToggleAi: () => void
  usageOpen: boolean
  onToggleUsage: () => void
}

/**
 * Slim, always-collapsed rail shown when `layout === 'top_tabs'`. Hosts the
 * brand mark, the workspace pill, and the bookmarks/VPN/settings controls.
 * Pinned tabs live in the top strip in this layout. Expands only while the
 * workspace popover is open.
 */
export function TopTabsRail({
  state,
  side,
  onOpenSettings,
  onOpenHistory,
  onOpenCanvases,
  aiAvailable,
  aiOpen,
  onToggleAi,
  usageOpen,
  onToggleUsage,
}: TopTabsRailProps) {
  const [workspacePopoverOpen, setWorkspacePopoverOpen] = useState(false)
  // Hold the expansion briefly after the popover closes.
  const [linger, setLinger] = useState(false)
  const prevOpenRef = useRef(false)
  useEffect(() => {
    const was = prevOpenRef.current
    prevOpenRef.current = workspacePopoverOpen
    if (!was || workspacePopoverOpen) return
    setLinger(true)
    const t = window.setTimeout(() => setLinger(false), 320)
    return () => window.clearTimeout(t)
  }, [workspacePopoverOpen])
  const expanded = workspacePopoverOpen || linger
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
        <DownloadsButton />
        {!activeWorkspace?.ephemeral && <BookmarksButton />}
        <VpnToggle workspace={activeWorkspace} />
        {aiAvailable && (
          <button
            type="button"
            className={cx('kb-circle-btn', 'kb-rail-settings', { 'is-active': aiOpen })}
            aria-label="Assistant"
            aria-pressed={aiOpen}
            onClick={onToggleAi}
          >
            <AssistantIcon />
          </button>
        )}
        <button
          type="button"
          className={cx('kb-circle-btn', 'kb-rail-settings', { 'is-active': usageOpen })}
          aria-label="Usage"
          aria-pressed={usageOpen}
          onClick={onToggleUsage}
        >
          <UsageIcon />
        </button>
        <button
          type="button"
          className="kb-circle-btn kb-rail-settings"
          aria-label="Canvases"
          onClick={onOpenCanvases}
        >
          <CanvasIcon />
        </button>
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
