import type { BrowserState } from '@/bindings/types'

import { TabWebViewHost } from '@/components/TabWebViewHost'

interface ContentProps {
  state: BrowserState
}

export function Content({ state }: ContentProps) {
  return (
    <main className="kb-main">
      <div className="kb-content">
        <TabWebViewHost state={state} />
      </div>
    </main>
  )
}
