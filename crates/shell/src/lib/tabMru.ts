/**
 * Most-recently-used tab tracking for this window: which tab was active
 * before the current one. Feeds Switch-to-last-tab (ping-pong) and the
 * close-time return, so closing the active tab lands on the previously used
 * tab instead of the tab_order neighbor core would pick (new tabs append at
 * the end, so the neighbor is rarely the opener).
 *
 * Module state is per-window by construction: each OS window runs its own
 * chrome webview and JS context.
 */

import type { TabId, WorkspaceId } from '@/bindings/types'

interface ActiveTabEntry {
  id: TabId
  workspaceId: WorkspaceId
}

let current: ActiveTabEntry | null = null
let previous: ActiveTabEntry | null = null

/** Record an activation; no-op when the active tab is unchanged. */
export function noteActiveTab(id: TabId | null, workspaceId: WorkspaceId | null): void {
  if (!id || !workspaceId) return
  if (current?.id === id) {
    // Same tab moved to another workspace: refresh so the same-workspace
    // guard in closeReturnTarget compares live data.
    if (current.workspaceId !== workspaceId) current = { id, workspaceId }
    return
  }
  previous = current
  current = { id, workspaceId }
}

/**
 * Drop entries whose tabs no longer exist (closes that never went through
 * closeTabAnimated: workspace deletion, bulk closes, other windows).
 */
export function pruneTabMru(isLive: (id: TabId) => boolean): void {
  if (previous && !isLive(previous.id)) previous = null
  if (current && !isLive(current.id)) current = null
}

/**
 * Drop a closing tab from the history so it can never become a return
 * target after it is gone.
 */
export function noteTabClosed(id: TabId): void {
  if (previous?.id === id) previous = null
  if (current?.id === id) current = null
}

/** The tab active before the current one (Switch-to-last-tab target). */
export function previousTab(): TabId | null {
  return previous?.id ?? null
}

/**
 * The tab to activate when closing tab `id`: the previously used tab, but
 * only when `id` is the currently active tab and the previous one lives in
 * the same workspace (a cross-workspace return would drag the window to
 * another workspace). Null means the caller should let core pick the
 * tab_order neighbor as before.
 */
export function closeReturnTarget(id: TabId): TabId | null {
  if (current?.id !== id) return null
  if (!previous || previous.id === id) return null
  if (previous.workspaceId !== current.workspaceId) return null
  return previous.id
}
