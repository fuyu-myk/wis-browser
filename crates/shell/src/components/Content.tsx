import { useRef } from 'react'
import type { AiAskRequest } from '@/lib/aiAsk'

import type { BrowserState, TabId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { isChromeUrl } from '@/lib/url'
import type { AiPageSource } from '@/hooks/useAiChat'

import { AiPanel } from '@/components/AiPanel'
import { TabWebViewHost } from '@/components/TabWebViewHost'
import { UsagePanel } from '@/components/UsagePanel'

interface ContentProps {
  state: BrowserState
  aiOpen: boolean
  /** A question from the page menu, seeded into the panel on arrival. */
  aiAsk: AiAskRequest | null
  onCloseAi: () => void
  usageOpen: boolean
  onCloseUsage: () => void
  onOpenSettings: () => void
  onDiscardedChange?: (discarded: Set<TabId>) => void
}

export function Content({
  state,
  aiOpen,
  aiAsk,
  onCloseAi,
  usageOpen,
  onCloseUsage,
  onOpenSettings,
  onDiscardedChange,
}: ContentProps) {
  const activeWorkspace = state.workspaces.find(
    (w) => w.id === state.active_workspace_id,
  )
  const isSplit = activeWorkspace?.pane_tree != null

  const activeTab =
    activeWorkspace?.active_tab_id != null
      ? state.tabs.find((t) => t.id === activeWorkspace.active_tab_id) ?? null
      : null
  const page: AiPageSource | null =
    activeTab && !isChromeUrl(activeTab.url) && activeTab.load_error === null
      ? { id: activeTab.id, url: activeTab.url, title: activeTab.title }
      : null

  const { layout, top_tabs_rail_side: railSide } = state.settings
  const aiSide =
    layout === 'sidebar_right' || (layout === 'top_tabs' && railSide === 'right')
      ? 'left'
      : 'right'

  const aiWebAccess =
    state.settings.ai.web_access && state.settings.ai.provider === 'open_ai_compatible'

  // Kept mounted after the first open.
  const aiMountedRef = useRef(false)
  if (aiOpen) aiMountedRef.current = true

  return (
    <main className={cx('kb-main', { 'has-ai': aiOpen, 'has-usage': usageOpen })}>
      <div className={cx('kb-content', { 'is-split': isSplit })}>
        <TabWebViewHost state={state} onDiscardedChange={onDiscardedChange} />
      </div>
      <UsagePanel
        open={usageOpen}
        side={aiSide === 'right' ? 'left' : 'right'}
        tracking={state.settings.usage_tracking}
        onClose={onCloseUsage}
        onOpenSettings={onOpenSettings}
      />
      {aiMountedRef.current && (
        <AiPanel
          open={aiOpen}
          ask={aiAsk}
          side={aiSide}
          workspaceId={activeWorkspace?.id ?? null}
          page={page}
          model={state.settings.ai.model}
          webAccess={aiWebAccess}
          onClose={onCloseAi}
          onOpenSettings={onOpenSettings}
        />
      )}
    </main>
  )
}
