import { useCallback, useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'

import type { BrowserState, Tab, TabId, UserSettings } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { HISTORY_URL, SETTINGS_URL } from '@/lib/url'
import { useBookmarksOverlay } from '@/hooks/useBookmarksOverlay'
import { useClearDataOverlay } from '@/hooks/useClearDataOverlay'
import { useDownloadsOverlay } from '@/hooks/useDownloadsOverlay'
import { useCommandPaletteOverlay } from '@/hooks/useCommandPaletteOverlay'
import { useDockIcon } from '@/hooks/useDockIcon'
import { useWindowPickerOverlay } from '@/hooks/useWindowPickerOverlay'
import { useNavGuardOverlay } from '@/hooks/useNavGuardOverlay'
import { useOpenInTab } from '@/hooks/useOpenInTab'
import { usePeekOverlay } from '@/hooks/usePeekOverlay'
import { useScreenshotToastOverlay } from '@/hooks/useScreenshotToastOverlay'
import { useScreenshotRegionOverlay } from '@/hooks/useScreenshotRegionOverlay'
import { useKeyActions } from '@/hooks/useKeyActions'
import { useNavigateOrCreateTab } from '@/hooks/useNavigateOrCreateTab'
import { useSwipeAccentSync } from '@/hooks/useSwipeAccentSync'
import { useWorkspaceBackground } from '@/hooks/useWorkspaceBackground'

import { Content } from '@/components/Content'
import { DiscardedTabsContext } from '@/components/DiscardedTabsContext'
import { NavRow } from '@/components/NavRow'
import { Sidebar } from '@/components/Sidebar'
import { TopBar } from '@/components/TopBar'
import { TopTabsRail } from '@/components/TopTabsRail'
import { FloatingUrlBar } from '@/components/UrlBar'
import { prefetchWidgetFavicons } from '@/components/WidgetFavicon'

interface WindowProps {
  state: BrowserState
  settings: UserSettings
}

export function Window({ state, settings }: WindowProps) {
  const [sidebarExpanded, setSidebarExpanded] = useState(false)
  const [discardedTabs, setDiscardedTabs] = useState<ReadonlySet<TabId>>(new Set())
  const onDiscardedChange = useCallback(
    (next: Set<TabId>) => setDiscardedTabs(next),
    [],
  )
  useKeyActions()
  useCommandPaletteOverlay()
  useWindowPickerOverlay()
  useClearDataOverlay()
  usePeekOverlay()
  useOpenInTab()
  useNavGuardOverlay()
  useBookmarksOverlay()
  useDownloadsOverlay()
  useScreenshotToastOverlay()
  useScreenshotRegionOverlay()

  const layoutClass = `layout-${settings.layout.replace('_', '-')}`
  const isSidebar = settings.layout !== 'top_tabs'
  const collapsible = isSidebar && settings.sidebar_collapsible
  const glassOn = settings.glass_enabled
  const showWindowTopUrl = isSidebar
  const railSide = settings.top_tabs_rail_side
  const railClass = !isSidebar ? `has-rail rail-${railSide}` : ''

  const activeWorkspace = state.workspaces.find((w) => w.id === state.active_workspace_id)
  const activeTab: Tab | null =
    activeWorkspace?.active_tab_id != null
      ? state.tabs.find((t) => t.id === activeWorkspace.active_tab_id) ?? null
      : null

  useWorkspaceBackground(activeWorkspace ?? null)

  const navigateOrCreate = useNavigateOrCreateTab(activeWorkspace?.id ?? null, activeTab)
  const openSettings = () => navigateOrCreate(SETTINGS_URL)
  const openHistory = () => navigateOrCreate(HISTORY_URL)

  const isPrivate = activeWorkspace?.ephemeral ?? false
  useDockIcon(isPrivate)
  useSwipeAccentSync(settings)

  // Warm the favicon cache for sidebar widgets on startup (and when the set
  // changes).
  const widgetUrlsKey = settings.sidebar_widgets.map((w) => w.url).join('\n')
  useEffect(() => {
    prefetchWidgetFavicons(widgetUrlsKey ? widgetUrlsKey.split('\n') : [])
  }, [widgetUrlsKey])

  return (
    <DiscardedTabsContext.Provider value={discardedTabs}>
    <div
      className={cx('kb-window', layoutClass, railClass, {
        collapsible,
        'glass-on': glassOn,
        'has-window-top-url': showWindowTopUrl,
        'has-window-top': isSidebar,
      })}
    >
      {isSidebar && (
        <div className="kb-window-top" data-tauri-drag-region>
          <NavRow activeTab={activeTab} circular />
          <div className="kb-urlpill-wrap">
            <FloatingUrlBar
              activeTab={activeTab}
              fallbackWorkspaceId={activeWorkspace?.id ?? null}
              inline
            />
          </div>
        </div>
      )}

      {settings.layout === 'top_tabs' ? (
        <>
          <TopBar state={state} onOpenSettings={openSettings} />
          <TopTabsRail
            state={state}
            side={railSide}
            onOpenSettings={openSettings}
            onOpenHistory={openHistory}
          />
        </>
      ) : (
        <Sidebar
          state={state}
          settings={settings}
          expanded={sidebarExpanded}
          setExpanded={setSidebarExpanded}
          onOpenHistory={openHistory}
          onOpenSettings={openSettings}
        />
      )}
      <Content state={state} onDiscardedChange={onDiscardedChange} />
      <WindowResizeEdges />
    </div>
    </DiscardedTabsContext.Provider>
  )
}

// Thin resize handles around the window perimeter.
const RESIZE_EDGES = [
  'North',
  'South',
  'East',
  'West',
  'NorthEast',
  'NorthWest',
  'SouthEast',
  'SouthWest',
] as const

const EDGE_CLASS: Record<(typeof RESIZE_EDGES)[number], string> = {
  North: 'n',
  South: 's',
  East: 'e',
  West: 'w',
  NorthEast: 'ne',
  NorthWest: 'nw',
  SouthEast: 'se',
  SouthWest: 'sw',
}

function WindowResizeEdges() {
  return (
    <>
      {RESIZE_EDGES.map((dir) => (
        <div
          key={dir}
          className={`kb-resize kb-resize-${EDGE_CLASS[dir]}`}
          onPointerDown={(e) => {
            if (e.button !== 0) return
            void getCurrentWindow().startResizeDragging(dir).catch(() => undefined)
          }}
        />
      ))}
    </>
  )
}
