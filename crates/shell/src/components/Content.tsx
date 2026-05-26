import type { BrowserState } from '@/bindings/types'

import { TabWebViewHost } from '@/components/TabWebViewHost'

interface ContentProps {
  state: BrowserState
  /** When true, the native tab webview is parked off-screen so a
   *  full-window HTML modal (e.g. settings) is visible. */
  suppressWebview?: boolean
}

export function Content({ state, suppressWebview }: ContentProps) {
  return (
    <main className="kb-main">
      <div className="kb-content">
        <TabWebViewHost state={state} suppressWebview={suppressWebview} />
      </div>
    </main>
  )
}
