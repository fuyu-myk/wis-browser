/**
 * Choices the clear-data dialog offers: how far back a wipe reaches, and which
 * slices of site data it takes.
 */

import type { SiteDataKind } from '@/bindings/types'

export type ClearRange = 'hour' | 'day' | 'week' | 'all'

export const CLEAR_RANGES: ReadonlyArray<{ value: ClearRange; label: string }> = [
  { value: 'hour', label: '1 hour' },
  { value: 'day', label: '24 hours' },
  { value: 'week', label: '7 days' },
  { value: 'all', label: 'All time' },
]

const RANGE_SECONDS: Record<Exclude<ClearRange, 'all'>, number> = {
  hour: 60 * 60,
  day: 24 * 60 * 60,
  week: 7 * 24 * 60 * 60,
}

/** The unix second a clear of `range` reaches back to, or null for all time. */
export function clearSince(range: ClearRange, nowMs: number = Date.now()): number | null {
  if (range === 'all') return null
  return Math.floor(nowMs / 1000) - RANGE_SECONDS[range]
}

export const ALL_SITE_DATA_KINDS: ReadonlyArray<SiteDataKind> = ['cookies', 'cache', 'storage']

export const SITE_DATA_KIND_ROWS: ReadonlyArray<{
  kind: SiteDataKind
  label: string
  hint: string
}> = [
  { kind: 'cookies', label: 'Cookies', hint: 'Signs you out of sites' },
  { kind: 'cache', label: 'Cached files', hint: 'Pages and images kept for faster loading' },
  { kind: 'storage', label: 'Site storage', hint: 'Local storage, databases and service workers' },
]
