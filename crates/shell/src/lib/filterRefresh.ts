export const FILTER_REFRESH_PROGRESS_EVENT = 'wis-filter-refresh-progress'

/** `done` of `total` steps: one per list, then applying them. */
export interface FilterRefreshProgress {
  done: number
  total: number
}

export function refreshProgressLine({ done, total }: FilterRefreshProgress): string {
  const lists = total - 1
  return done < lists ? `Fetching list ${done + 1} of ${lists}...` : 'Applying to open tabs...'
}
