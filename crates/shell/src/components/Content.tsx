import type { BrowserState } from '@/bindings/types'
import { cx } from '@/lib/cx'

import { TabWebViewHost } from '@/components/TabWebViewHost'

interface ContentProps {
  state: BrowserState
}

export function Content({ state }: ContentProps) {
  const activeWorkspace = state.workspaces.find(
    (w) => w.id === state.active_workspace_id,
  )
  const isSplit = activeWorkspace?.pane_tree != null

  return (
    <main className="kb-main">
      <div className={cx('kb-content', { 'is-split': isSplit })}>
        <TabWebViewHost state={state} />
      </div>
    </main>
  )
}
