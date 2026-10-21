import { useEffect, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { FILTER_REFRESH_PROGRESS_EVENT, type FilterRefreshProgress } from '@/lib/filterRefresh'

/** The refresh under way, or `null` once it has finished. */
export function useFilterRefreshProgress(): FilterRefreshProgress | null {
  const [progress, setProgress] = useState<FilterRefreshProgress | null>(null)

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<FilterRefreshProgress>(FILTER_REFRESH_PROGRESS_EVENT, (e) => {
      setProgress(e.payload.done < e.payload.total ? e.payload : null)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  return progress
}
