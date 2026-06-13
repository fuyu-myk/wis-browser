import { useEffect, useRef, useState, type MouseEvent } from 'react'

import type { BrowserState, Tab, UserSettings } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { HistoryIcon, SettingsIcon } from '@/lib/icons'

import { BookmarksButton } from '@/components/BookmarksButton'
import { Brand } from '@/components/Brand'
import { DownloadsButton } from '@/components/DownloadsButton'
import { PinnedTabs } from '@/components/PinnedTabs'
import { TabList } from '@/components/TabList'
import { VpnToggle } from '@/components/VpnToggle'
import { WidgetDock } from '@/components/WidgetDock'
import { WorkspacePill } from '@/components/WorkspacePill'

interface SidebarProps {
  state: BrowserState
  settings: UserSettings
  /** Lifted to <Window> so the URL bar can also request expansion. */
  expanded: boolean
  setExpanded: (v: boolean) => void
  onOpenHistory: () => void
  onOpenSettings: () => void
}

// Expands on edge hover, empty-area click, or URL bar activation when
// `sidebar_collapsible` is on.
export function Sidebar({
  state,
  settings,
  expanded,
  setExpanded,
  onOpenHistory,
  onOpenSettings,
}: SidebarProps) {
  const activeWorkspace = state.workspaces.find((w) => w.id === state.active_workspace_id)
  const tabs: Tab[] = activeWorkspace
    ? activeWorkspace.tab_order
        .map((id) => state.tabs.find((t) => t.id === id))
        .filter((t): t is Tab => t !== undefined)
    : []
  const activeTab = tabs.find((t) => t.id === activeWorkspace?.active_tab_id) ?? null

  const collapsible = settings.sidebar_collapsible

  const asideRef = useRef<HTMLElement>(null)

  const [workspacePopoverOpen, setWorkspacePopoverOpen] = useState(false)
  const [tabMenuOpen, setTabMenuOpen] = useState(false)
  const [folderEditing, setFolderEditing] = useState(false)
  const keepOpen = workspacePopoverOpen || tabMenuOpen || folderEditing
  // A live menu/popover/edit keeps the sidebar pinned open. Tracked via a ref so
  // the window-blur handler reads the current value without re-subscribing.
  const keepOpenRef = useRef(false)
  keepOpenRef.current = keepOpen
  useEffect(() => {
    if (keepOpen) setExpanded(true)
  }, [keepOpen, setExpanded])

  // Broadcast collapse so open folder edit popovers discard and close.
  useEffect(() => {
    if (!expanded) window.dispatchEvent(new CustomEvent('wis-sidebar-collapsed'))
  }, [expanded])

  const sidebarSide: 'left' | 'right' =
    settings.layout === 'sidebar_right' ? 'right' : 'left'
  useEffect(() => {
    if (!collapsible || expanded) return
    const EDGE_PX = 8
    const onMove = (e: globalThis.MouseEvent) => {
      const atEdge =
        sidebarSide === 'left'
          ? e.clientX <= EDGE_PX
          : e.clientX >= window.innerWidth - EDGE_PX
      if (atEdge) setExpanded(true)
    }
    document.addEventListener('mousemove', onMove)
    return () => document.removeEventListener('mousemove', onMove)
  }, [collapsible, expanded, sidebarSide, setExpanded])

  useEffect(() => {
    if (!collapsible || !expanded) return
    // Esc and explicit click-out close the workspace popover and
    // collapse the sidebar in a single gesture.
    const collapse = () => {
      window.dispatchEvent(new CustomEvent('wis-close-workspace-popover'))
      setWorkspacePopoverOpen(false)
      setExpanded(false)
    }
    const onPointerDown = (e: PointerEvent) => {
      const aside = asideRef.current
      if (!aside) return
      const target = e.target as Element | null
      if (aside.contains(target)) return
      if (target?.closest?.('.kb-ws-pop')) return
      if (target?.closest?.('.kb-split-menu')) return
      collapse()
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        collapse()
      }
    }
    const onBlur = () => {
      if (keepOpenRef.current) return
      collapse()
    }
    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    window.addEventListener('blur', onBlur)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
      window.removeEventListener('blur', onBlur)
    }
  }, [collapsible, expanded, setExpanded])

  const handleAsideClick = (e: MouseEvent<HTMLElement>) => {
    if (!collapsible || expanded) return
    if (e.target === e.currentTarget) {
      setExpanded(true)
    }
  }

  return (
    <aside
      ref={asideRef}
      className={cx('kb-side', { 'is-expanded': expanded })}
      onMouseLeave={() => collapsible && !keepOpen && setExpanded(false)}
      onClick={handleAsideClick}
    >
      {collapsible && (
        <div
          className="kb-side-edge"
          aria-hidden
          onMouseEnter={() => setExpanded(true)}
        />
      )}
      <Brand />
      <WorkspacePill state={state} onOpenChange={setWorkspacePopoverOpen} />
      {activeWorkspace && (
        <PinnedTabs
          variant="sidebar"
          workspaceId={activeWorkspace.id}
          tabs={tabs}
          pinnedTabIds={activeWorkspace.pinned_tab_ids}
          activeTabId={activeTab?.id ?? null}
          selectedFolderId={activeWorkspace.selected_folder_id ?? null}
          onExpand={() => setExpanded(true)}
          onMenuOpenChange={setTabMenuOpen}
        />
      )}
      <TabList
        workspaceId={activeWorkspace?.id ?? ('' as never)}
        tabs={tabs}
        activeTabId={activeTab?.id ?? null}
        pinnedTabIds={activeWorkspace?.pinned_tab_ids ?? []}
        folders={activeWorkspace?.tab_folders ?? []}
        selectedFolderId={activeWorkspace?.selected_folder_id ?? null}
        onSplitMenuOpenChange={setTabMenuOpen}
        onFolderEditingChange={setFolderEditing}
        workspaceOrder={state.workspaces.map((w) => w.id)}
      />
      <WidgetDock widgets={settings.sidebar_widgets} variant="sidebar" />
      <div className="kb-side-footer">
        <DownloadsButton />
        {!activeWorkspace?.ephemeral && <BookmarksButton />}
        <VpnToggle workspace={activeWorkspace ?? null} />
        <button
          type="button"
          className="kb-circle-btn"
          aria-label="History"
          title="History"
          onClick={onOpenHistory}
        >
          <HistoryIcon />
        </button>
        <button
          type="button"
          className="kb-circle-btn"
          aria-label="Settings"
          title="Settings"
          onClick={onOpenSettings}
        >
          <SettingsIcon />
        </button>
      </div>
    </aside>
  )
}
