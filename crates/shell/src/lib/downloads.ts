/** Shared constants and pure helpers for the downloads UI (button, popup
 * surface, full page). */

import type { DownloadItem } from '@/bindings/types'

/** DOM event the downloads glyph dispatches to toggle the popup overlay. */
export const DOWNLOADS_OPEN_EVENT = 'wis:downloads-open'
/** Tauri event the popup broadcasts after closing itself. */
export const DOWNLOADS_CLOSED_EVENT = 'wis-downloads-closed'
/** Tauri event the backend emits on any download change. */
export const DOWNLOADS_CHANGED_EVENT = 'downloads-changed'

/** React Query cache key for the merged downloads list. */
export const DOWNLOADS_KEY = ['downloads'] as const

/** Entries shown in the popup before "View all downloads". */
export const POPUP_LIMIT = 8

export function isInProgress(item: DownloadItem): boolean {
  return item.status === 'in_progress'
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let value = bytes
  let unit = ''
  for (const next of units) {
    value /= 1024
    unit = next
    if (value < 1024) break
  }
  return `${value >= 100 ? Math.round(value) : value.toFixed(1)} ${unit}`
}

/** Host of the source URL, for the row's secondary line. */
export function downloadHost(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, '')
  } catch {
    return url
  }
}

/** Case-insensitive substring match over filename, url, and host. */
export function matchesDownloadSearch(item: DownloadItem, search: string): boolean {
  const needle = search.trim().toLowerCase()
  if (!needle) return true
  return (
    item.filename.toLowerCase().includes(needle) ||
    item.url.toLowerCase().includes(needle) ||
    downloadHost(item.url).toLowerCase().includes(needle)
  )
}

export function statusLabel(item: DownloadItem): string {
  switch (item.status) {
    case 'in_progress':
      return `Downloading, ${formatBytes(item.receivedBytes)} so far`
    case 'completed':
      return formatBytes(item.receivedBytes)
    case 'failed':
      return 'Failed'
    case 'cancelled':
      return 'Cancelled'
  }
}
