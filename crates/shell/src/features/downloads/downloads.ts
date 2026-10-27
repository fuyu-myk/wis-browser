/** Shared constants and pure helpers for the downloads UI (button, popup
 * surface, full page). */

import type { DownloadId, DownloadItem, DownloadStatus } from '@/bindings/types'

/** DOM event the downloads glyph dispatches to toggle the popup overlay. */
export const DOWNLOADS_OPEN_EVENT = 'wis:downloads-open'
/** Tauri event the popup broadcasts after closing itself. */
export const DOWNLOADS_CLOSED_EVENT = 'wis-downloads-closed'
/** Tauri event the chrome emits each time the popup overlay is shown. */
export const DOWNLOADS_SHOWN_EVENT = 'wis-downloads-shown'
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
  // Round what is shown, with the unit chosen after rounding.
  const shown = (v: number) => (v >= 100 ? Math.round(v) : Math.round(v * 10) / 10)
  let value = bytes / 1024
  let unit = 0
  while (shown(value) >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit++
  }
  const rounded = shown(value)
  return `${rounded >= 100 ? rounded : rounded.toFixed(1)} ${units[unit]}`
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

/**
 * What to say for downloads that finished or failed since `before`. A
 * download unseen before counts as pending.
 */
export function downloadAnnouncement(
  before: ReadonlyMap<DownloadId, DownloadStatus>,
  now: ReadonlyArray<Pick<DownloadItem, 'id' | 'filename' | 'status'>>,
): string | null {
  const lines = now.flatMap((d) => {
    const was = before.get(d.id)
    if ((was !== undefined && was !== 'in_progress') || was === d.status) return []
    if (d.status === 'completed') return [`Download complete: ${d.filename}`]
    if (d.status === 'failed') return [`Download failed: ${d.filename}`]
    return []
  })
  return lines.length > 0 ? lines.join('. ') : null
}
