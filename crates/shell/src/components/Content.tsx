import type { BrowserState, TabId } from '@/bindings/types'
import { cx } from '@/lib/cx'

import { TabWebViewHost } from '@/components/TabWebViewHost'

interface ContentProps {
  state: BrowserState
  onDiscardedChange?: (discarded: Set<TabId>) => void
}

export function Content({ state, onDiscardedChange }: ContentProps) {
  const activeWorkspace = state.workspaces.find(
    (w) => w.id === state.active_workspace_id,
  )
  const isSplit = activeWorkspace?.pane_tree != null

  return (
    <main className="kb-main">
      <div className={cx('kb-content', { 'is-split': isSplit })}>
        <TabWebViewHost state={state} onDiscardedChange={onDiscardedChange} />
      </div>
    </main>
  )
}
