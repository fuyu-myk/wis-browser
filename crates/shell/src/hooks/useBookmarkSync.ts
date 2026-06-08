/**
 * Keep bookmark queries in sync across webviews. The star (main chrome), the
 * bookmarks popup, and the folder picker each run in their own webview with a
 * separate React Query cache, so a mutation in one would otherwise not refresh
 * the others. Every bookmark mutation broadcasts `bookmarks-changed`; this hook
 * invalidates the bookmark/folder queries in the current webview on that event.
 */

import { useEffect } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useQueryClient } from '@tanstack/react-query'

export function useBookmarkSync(): void {
  const qc = useQueryClient()
  useEffect(() => {
    let cancelled = false
    let unlisten: UnlistenFn | null = null
    void listen('bookmarks-changed', () => {
      void qc.invalidateQueries({
        predicate: (q) => {
          const k = q.queryKey[0]
          return k === 'bookmarks' || k === 'bookmark-folders'
        },
      })
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [qc])
}
