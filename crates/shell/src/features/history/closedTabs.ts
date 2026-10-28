import type { ClosedTabRecord, WorkspaceId } from '@/bindings/types'
import { hostOf } from '@/features/history/dayGroups'
import { isCanvasUrl, isChromeUrl, tabDisplayName } from '@/lib/url'

export const CLOSED_TABS_CHANGED_EVENT = 'closed-tabs-changed'

/** `workspace` null shows every workspace's. */
export function visibleClosedTabs(
  entries: ReadonlyArray<ClosedTabRecord>,
  workspace: WorkspaceId | null,
  query: string,
): ClosedTabRecord[] {
  const needle = query.trim().toLowerCase()
  return entries.filter(
    (entry) =>
      (workspace === null || entry.workspace_id === workspace) &&
      (needle === '' ||
        entry.title.toLowerCase().includes(needle) ||
        entry.url.toLowerCase().includes(needle)),
  )
}

/** A closed tab's name: a Wis page or canvas as its tab showed it, else its title or site. */
export function closedTabTitle(entry: { title: string; url: string }): string {
  if (isChromeUrl(entry.url) || isCanvasUrl(entry.url)) return tabDisplayName(entry)
  return entry.title.trim() || hostOf(entry.url) || entry.url
}
