import { createContext, useContext } from 'react'

import type { TabId } from '@/bindings/types'

/**
 * Tabs whose native webview has been discarded to save memory ("asleep").
 * Runtime-only: never persisted. The tab list dims these so the user can tell
 * a page will reload on next focus.
 */
export const DiscardedTabsContext = createContext<ReadonlySet<TabId>>(new Set())

export function useDiscardedTabs(): ReadonlySet<TabId> {
  return useContext(DiscardedTabsContext)
}
