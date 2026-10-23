import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { BrowserState, Tab } from '@/bindings/types'
import { FOLDER_ACCENTS } from '@/lib/accents'
import { FolderPlusIcon, PlusIcon } from '@/lib/icons'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

import { HorizontalTabs } from '@/features/tabs/components/HorizontalTabs'
import { NavRow } from '@/components/NavRow'
import { FloatingUrlBar } from '@/features/urlbar/UrlBar'
import { WindowControls } from '@/components/WindowControls'

interface TopBarProps {
  state: BrowserState
  onOpenSettings?: () => void
}

/**
 * The horizontal chrome strip shown when `layout === 'top_tabs'`.
 *
 * Single layout: `url_bar_position` is ignored in this mode.
 * Two-row structure:
 *   row 1: traffic-light reserve | nav circles | URL pill | +
 *   row 2: centered compact tab strip (Safari-style: proportional,
 *           shrink, then horizontal-scroll)
 */
export function TopBar({ state }: TopBarProps) {
  const qc = useQueryClient()

  const activeWorkspace = state.workspaces.find((w) => w.id === state.active_workspace_id)
  const tabs: Tab[] = activeWorkspace
    ? activeWorkspace.tab_order
        .map((id) => state.tabs.find((t) => t.id === id))
        .filter((t): t is Tab => t !== undefined)
    : []
  const activeTab = tabs.find((t) => t.id === activeWorkspace?.active_tab_id) ?? null

  const createTab = useMutation({
    // The explicit "+ New tab" button always makes a loose, top-level tab; only
    // Cmd+T and link/bookmark opens honour the selected folder.
    mutationFn: () => {
      if (!activeWorkspace) throw new Error('no active workspace')
      return commands.createLooseTab(activeWorkspace.id, 'about:blank')
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  const createFolder = useMutation({
    mutationFn: async () => {
      if (!activeWorkspace) throw new Error('no active workspace')
      const folders = activeWorkspace.tab_folders
      const folder = await commands.createTabFolder(
        activeWorkspace.id,
        'New folder',
        FOLDER_ACCENTS[folders.length % FOLDER_ACCENTS.length],
      )
      if (activeTab) await commands.assignTabToFolder(activeTab.id, folder.id)
      return folder
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  return (
    <div className="kb-topbar">
      <div className="kb-topbar-row primary" data-tauri-drag-region>
        <NavRow activeTab={activeTab} circular />
        <div className="kb-urlpill-wrap">
          <FloatingUrlBar
            activeTab={activeTab}
            fallbackWorkspaceId={activeWorkspace?.id ?? null}
            inline
          />
        </div>
        <button
          type="button"
          className="kb-circle-btn"
          aria-label="New tab"
          onClick={() => createTab.mutate()}
          disabled={!activeWorkspace || createTab.isPending}
        >
          <PlusIcon />
        </button>
        <button
          type="button"
          className="kb-circle-btn"
          aria-label="New folder"
          title="New folder"
          onClick={() => createFolder.mutate()}
          disabled={!activeWorkspace || createFolder.isPending}
        >
          <FolderPlusIcon />
        </button>
        <WindowControls />
      </div>
      <div className="kb-topbar-row tabs">
        <HorizontalTabs
          workspaceId={activeWorkspace?.id ?? ('' as never)}
          tabs={tabs}
          activeTabId={activeTab?.id ?? null}
          pinnedTabIds={activeWorkspace?.pinned_tab_ids ?? []}
          folders={activeWorkspace?.tab_folders ?? []}
          selectedFolderId={activeWorkspace?.selected_folder_id ?? null}
          railSide={state.settings.top_tabs_rail_side}
          workspaceOrder={state.workspaces.map((w) => w.id)}
          themeAccent={state.settings.accent}
          integrateUrlBar={false}
          compact
          maxExpandedFolders={state.settings.top_tabs_max_expanded_folders}
          paneTree={activeWorkspace?.pane_tree ?? null}
        />
      </div>
    </div>
  )
}
