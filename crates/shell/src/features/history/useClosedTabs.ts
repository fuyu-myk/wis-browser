import { useEffect } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { ClosedTabRecord } from '@/bindings/types'
import { CLOSED_TABS_CHANGED_EVENT } from '@/features/history/closedTabs'
import { CLOSED_TAB_LIMIT } from '@/lib/palette'

const CLOSED_TABS_KEY = ['closed-tabs'] as const

/** Every closed tab Wis keeps, newest first, kept current while `shown`. */
export function useClosedTabs(shown: boolean): { entries: ClosedTabRecord[]; isLoading: boolean } {
  const qc = useQueryClient()
  const { data: entries = [], isLoading } = useQuery({
    queryKey: CLOSED_TABS_KEY,
    queryFn: () => commands.listClosedTabs(CLOSED_TAB_LIMIT),
    staleTime: 0,
    refetchOnMount: 'always',
    enabled: shown,
  })

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen(CLOSED_TABS_CHANGED_EVENT, () => {
      void qc.invalidateQueries({ queryKey: CLOSED_TABS_KEY })
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [qc])

  return { entries, isLoading }
}
