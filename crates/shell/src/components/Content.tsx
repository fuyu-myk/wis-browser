import { useRef } from 'react'
import type { AiAskRequest } from '@/features/ai/lib/aiAsk'

import { commands } from '@/bindings/commands'
import type { BrowserState, TabId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { isChromeUrl } from '@/lib/url'
import type { AiPageSource } from '@/features/ai/hooks/useAiChat'

import { AiPanel } from '@/features/ai/components/AiPanel'
import { TabWebViewHost } from '@/components/TabWebViewHost'
import { UsagePanel } from '@/features/usage/components/UsagePanel'

interface ContentProps {
  state: BrowserState
  aiOpen: boolean
  /** A question from the page menu, seeded into the panel on arrival. */
  aiAsk: AiAskRequest | null
  onCloseAi: () => void
  usageOpen: boolean
  onCloseUsage: () => void
  onOpenUsagePage: () => void
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
  onOpenUsagePage,
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

  const aiWebAccess = state.settings.ai.web_access
  const aiLocalData = state.settings.ai.local_data
  const aiTabActions = aiLocalData && state.settings.ai.tab_actions
  const chooseAiModel = async (model: string) => {
    await commands.updateSettings({ ai: { ...state.settings.ai, model } })
  }

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
        weekStart={state.settings.week_start}
        onClose={onCloseUsage}
        onOpenSettings={onOpenSettings}
        onOpenPage={onOpenUsagePage}
      />
      {aiMountedRef.current && (
        <AiPanel
          open={aiOpen}
          ask={aiAsk}
          side={aiSide}
          workspaceId={activeWorkspace?.id ?? null}
          page={page}
          model={state.settings.ai.model}
          onChooseModel={chooseAiModel}
          webAccess={aiWebAccess}
          localData={aiLocalData}
          tabActions={aiTabActions}
          onClose={onCloseAi}
          onOpenSettings={onOpenSettings}
        />
      )}
    </main>
  )
}
