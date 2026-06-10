/**
 * The merged downloads list (this session + persisted history), kept live by
 * invalidating on the backend's `downloads-changed` event. Shared by the
 * chrome button (active indicator), the popup surface, and the full page;
 * each webview has its own React Query cache, so each mounts this hook.
 */

import { useEffect } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { DownloadItem } from '@/bindings/types'
import { DOWNLOADS_CHANGED_EVENT, DOWNLOADS_KEY } from '@/lib/downloads'

interface UseDownloadsReturn {
  downloads: DownloadItem[]
  /** Truthy while any download is in flight (drives the button indicator). */
  active: boolean
}

export function useDownloads(): UseDownloadsReturn {
  const qc = useQueryClient()

  const { data: downloads = [] } = useQuery({
    queryKey: DOWNLOADS_KEY,
    queryFn: () => commands.listDownloads(),
    staleTime: 0,
    refetchOnMount: 'always',
  })

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen(DOWNLOADS_CHANGED_EVENT, () => {
      void qc.invalidateQueries({ queryKey: DOWNLOADS_KEY })
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [qc])

  return { downloads, active: downloads.some((d) => d.status === 'in_progress') }
}
