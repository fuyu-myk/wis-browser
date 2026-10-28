import { useEffect, useRef } from 'react'

import type { Tab, TabId } from '@/bindings/types'
import { withClosingGhosts } from '@/features/tabs/lib/tabClose'

/**
 * Optimistic close: the backend removes the tab immediately, while the strip
 * keeps a ghost row in place until the exit animation finishes.
 */
export function useClosingGhosts(tabs: Tab[], closingIds: ReadonlySet<TabId>): Tab[] {
  const prevRef = useRef<Tab[]>(tabs)
  const shown = withClosingGhosts(tabs, prevRef.current, closingIds)
  useEffect(() => {
    prevRef.current = shown
  })
  return shown
}
