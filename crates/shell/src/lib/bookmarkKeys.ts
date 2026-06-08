import type { WorkspaceId } from '@/bindings/types'

/** React Query keys for the per-workspace bookmark stores. */
export const bookmarkFoldersKey = (ws: WorkspaceId) => ['bookmark-folders', ws] as const
export const bookmarksKey = (ws: WorkspaceId) => ['bookmarks', ws] as const
