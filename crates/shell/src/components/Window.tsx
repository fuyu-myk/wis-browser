import { useRef, useState } from 'react'

import type { BrowserState, Tab, UserSettings } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { SettingsIcon } from '@/lib/icons'
import { SETTINGS_URL } from '@/lib/url'
import { useCommandPaletteOverlay } from '@/hooks/useCommandPaletteOverlay'
import { useKeyActions } from '@/hooks/useKeyActions'
import { useNavigateOrCreateTab } from '@/hooks/useNavigateOrCreateTab'

import { Content } from '@/components/Content'
import { NavRow } from '@/components/NavRow'
import { Sidebar } from '@/components/Sidebar'
import { TopBar } from '@/components/TopBar'
import { TopTabsRail } from '@/components/TopTabsRail'
import { FloatingUrlBar } from '@/components/UrlBar'

interface WindowProps {
  state: BrowserState
  settings: UserSettings
}

export function Window({ state, settings }: WindowProps) {
  const [sidebarExpanded, setSidebarExpanded] = useState(false)
  const sidebarUrlInputRef = useRef<HTMLInputElement>(null)
  useKeyActions()
  useCommandPaletteOverlay()

  const layoutClass = `layout-${settings.layout.replace('_', '-')}`
  const isSidebar = settings.layout !== 'top_tabs'
  const collapsible = isSidebar && settings.sidebar_collapsible
  const glassOn = settings.glass_enabled
  const urlAtTop = settings.url_bar_position === 'top'
  const showWindowTopUrl = isSidebar && urlAtTop
  const railSide = settings.top_tabs_rail_side
  const railClass = !isSidebar ? `has-rail rail-${railSide}` : ''

  const fixedSettingsAnchor: 'left' | 'right' =
    settings.layout === 'sidebar_right' ? 'right' : 'left'

  const activeWorkspace = state.workspaces.find((w) => w.id === state.active_workspace_id)
  const activeTab: Tab | null =
    activeWorkspace?.active_tab_id != null
      ? state.tabs.find((t) => t.id === activeWorkspace.active_tab_id) ?? null
      : null

  const navigateOrCreate = useNavigateOrCreateTab(activeWorkspace?.id ?? null, activeTab)
  const openSettings = () => navigateOrCreate(SETTINGS_URL)

  return (
    <div
      className={cx('kb-window', layoutClass, railClass, {
        collapsible,
        'glass-on': glassOn,
        'has-window-top-url': showWindowTopUrl,
        'has-window-top': isSidebar,
      })}
    >
      {isSidebar && (
        <div className="kb-window-top">
          <NavRow activeTab={activeTab} circular />
          {urlAtTop && (
            <div className="kb-urlpill-wrap">
              <FloatingUrlBar
                activeTab={activeTab}
                fallbackWorkspaceId={activeWorkspace?.id ?? null}
                inline
              />
            </div>
          )}
        </div>
      )}

      {settings.layout === 'top_tabs' ? (
        <>
          <TopBar state={state} onOpenSettings={openSettings} />
          <TopTabsRail
            state={state}
            side={railSide}
            onOpenSettings={openSettings}
          />
        </>
      ) : (
        <>
          <Sidebar
            state={state}
            settings={settings}
            expanded={sidebarExpanded}
            setExpanded={setSidebarExpanded}
            urlInputRef={sidebarUrlInputRef}
          />
          <button
            type="button"
            className={cx('kb-settings-fixed', `is-${fixedSettingsAnchor}`)}
            aria-label="Settings"
            onClick={openSettings}
          >
            <SettingsIcon />
          </button>
        </>
      )}
      <Content state={state} />
    </div>
  )
}
