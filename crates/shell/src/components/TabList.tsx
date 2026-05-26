import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { TabId, WorkspaceId, Tab } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { PlusIcon, XIcon } from '@/lib/icons'
import { tabDisplayName } from '@/lib/url'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

import { TabFavicon } from '@/components/TabFavicon'

interface TabListProps {
  workspaceId: WorkspaceId
  tabs: Tab[]
  activeTabId: TabId | null
}

/**
 * Vertical tab list shown inside the sidebar. The `TABS` group header is
 * followed by a `+ New tab` row, then the actual tab entries. Each tab
 * renders a coloured "fav" tile (favicon placeholder), title, and a
 * close affordance on hover.
 */
export function TabList({ workspaceId, tabs, activeTabId }: TabListProps) {
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

  return (
    <div className="kb-tabs">
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
      {tabs.map((tab) => (
        <div
          key={tab.id}
          className={cx('kb-tab', { active: tab.id === activeTabId, pinned: tab.pinned })}
          role="button"
          tabIndex={0}
          onClick={() => setActive.mutate(tab.id)}
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
