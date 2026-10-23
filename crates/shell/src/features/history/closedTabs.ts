import type { ClosedTabRecord, WorkspaceId } from '@/bindings/types'

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
