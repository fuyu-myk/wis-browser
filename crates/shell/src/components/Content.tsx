import { useRef } from 'react'

import type { BrowserState, TabId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { isChromeUrl } from '@/lib/url'
import type { AiPageSource } from '@/hooks/useAiChat'

import { AiPanel } from '@/components/AiPanel'
import { TabWebViewHost } from '@/components/TabWebViewHost'

interface ContentProps {
  state: BrowserState
  aiOpen: boolean
  onCloseAi: () => void
  onOpenSettings: () => void
  onDiscardedChange?: (discarded: Set<TabId>) => void
}

export function Content({
  state,
  aiOpen,
  onCloseAi,
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

  // Kept mounted after the first open.
  const aiMountedRef = useRef(false)
  if (aiOpen) aiMountedRef.current = true

  return (
    <main className={cx('kb-main', { 'has-ai': aiOpen })}>
      <div className={cx('kb-content', { 'is-split': isSplit })}>
        <TabWebViewHost state={state} onDiscardedChange={onDiscardedChange} />
      </div>
      {aiMountedRef.current && (
        <AiPanel
          open={aiOpen}
          side={aiSide}
          workspaceId={activeWorkspace?.id ?? null}
          page={page}
          model={state.settings.ai.model}
          onClose={onCloseAi}
          onOpenSettings={onOpenSettings}
        />
      )}
    </main>
  )
}
