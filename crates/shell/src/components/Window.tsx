import { useCallback, useEffect, useRef, useState } from 'react'
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
import { useExternalUrls } from '@/hooks/useExternalUrls'
import { useNavGuardOverlay } from '@/hooks/useNavGuardOverlay'
import { useOpenInTab } from '@/hooks/useOpenInTab'
import { usePageMenu } from '@/hooks/usePageMenu'
import { usePeekOverlay } from '@/hooks/usePeekOverlay'
import { useTabHoverPreview } from '@/hooks/useTabHoverPreview'
import { useScreenshotToastOverlay } from '@/hooks/useScreenshotToastOverlay'
import { useScreenshotRegionOverlay } from '@/hooks/useScreenshotRegionOverlay'
import { useToastOverlay } from '@/hooks/useToastOverlay'
import { useKeyActions } from '@/hooks/useKeyActions'
import { useKeybindCapture } from '@/hooks/useKeybindCapture'
import { useNavigateOrCreateTab } from '@/hooks/useNavigateOrCreateTab'
import { useSwipeAccentSync } from '@/hooks/useSwipeAccentSync'
import { useAiAvailable } from '@/hooks/useAiAvailable'
import { useAiAsk } from '@/hooks/useAiAsk'
import { useWorkspaceBackground } from '@/hooks/useWorkspaceBackground'

import { Content } from '@/components/Content'
import { DiscardedTabsContext } from '@/components/DiscardedTabsContext'
import { NavRow } from '@/components/NavRow'
import { Sidebar } from '@/components/Sidebar'
import { TopBar } from '@/components/TopBar'
import { TopTabsRail } from '@/components/TopTabsRail'
import { FloatingUrlBar } from '@/components/UrlBar'
import { prefetchWidgetFavicons } from '@/components/WidgetFavicon'
import { WindowControls } from '@/components/WindowControls'

interface WindowProps {
  state: BrowserState
  settings: UserSettings
}

export function Window({ state, settings }: WindowProps) {
  const [sidebarExpanded, setSidebarExpanded] = useState(false)
  const [aiOpen, setAiOpen] = useState(false)
  const [usageOpen, setUsageOpen] = useState(false)
  const [discardedTabs, setDiscardedTabs] = useState<ReadonlySet<TabId>>(new Set())
  const onDiscardedChange = useCallback(
    (next: Set<TabId>) => setDiscardedTabs(next),
    [],
  )
  useKeyActions()
  useKeybindCapture(settings.keybindings, { takeFocusOnMount: true })
  useCommandPaletteOverlay()
  useWindowPickerOverlay()
  useClearDataOverlay()
  usePeekOverlay()
  useTabHoverPreview()
  useOpenInTab()
  usePageMenu()
  useExternalUrls()
  useNavGuardOverlay()
  useBookmarksOverlay()
  useDownloadsOverlay()
  useScreenshotToastOverlay()
  useScreenshotRegionOverlay()
  useToastOverlay()

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

  const aiAvailable = useAiAvailable(settings.ai)
  const aiAsk = useAiAsk()
  useEffect(() => {
    if (!aiAvailable) setAiOpen(false)
  }, [aiAvailable])

  const openedAskRef = useRef(0)
  useEffect(() => {
    if (!aiAsk || aiAsk.seq <= openedAskRef.current) return
    openedAskRef.current = aiAsk.seq
    if (aiAvailable) setAiOpen(true)
  }, [aiAsk, aiAvailable])

  // Warm the favicon cache for sidebar widgets on startup (and when the set
  // changes).
  const widgetUrlsKey = settings.sidebar_widgets.map((w) => w.url).join('\n')
  useEffect(() => {
    const timer = window.setTimeout(() => {
      prefetchWidgetFavicons(widgetUrlsKey ? widgetUrlsKey.split('\n') : [])
    }, 4000)
    return () => window.clearTimeout(timer)
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
          <WindowControls />
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
            aiAvailable={aiAvailable}
            aiOpen={aiOpen}
            onToggleAi={() => setAiOpen((v) => !v)}
            usageOpen={usageOpen}
            onToggleUsage={() => setUsageOpen((v) => !v)}
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
          aiAvailable={aiAvailable}
          aiOpen={aiOpen}
          onToggleAi={() => setAiOpen((v) => !v)}
          usageOpen={usageOpen}
          onToggleUsage={() => setUsageOpen((v) => !v)}
        />
      )}
      <Content
        state={state}
        aiOpen={aiOpen}
        aiAsk={aiAsk}
        onCloseAi={() => setAiOpen(false)}
        usageOpen={usageOpen}
        onCloseUsage={() => setUsageOpen(false)}
        onOpenSettings={openSettings}
        onDiscardedChange={onDiscardedChange}
      />
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
