import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { BrowserState, Tab } from '@/bindings/types'
import { PlusIcon } from '@/lib/icons'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

import { HorizontalTabs } from '@/components/HorizontalTabs'
import { NavRow } from '@/components/NavRow'
import { FloatingUrlBar } from '@/components/UrlBar'

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
    mutationFn: () => {
      if (!activeWorkspace) throw new Error('no active workspace')
      return commands.createTab(activeWorkspace.id, 'about:blank')
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  return (
    <div className="kb-topbar">
      <div className="kb-topbar-row primary">
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
      </div>
      <div className="kb-topbar-row tabs">
        <HorizontalTabs
          tabs={tabs}
          activeTabId={activeTab?.id ?? null}
          integrateUrlBar={false}
          compact
        />
      </div>
    </div>
  )
}
