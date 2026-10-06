/**
 * Most-recently-used tab tracking for this window. Feeds Switch-to-last-tab
 * (ping-pong) and the close-time return, so closing the active tab lands on
 * the previously used tab instead of the tab_order neighbor core would pick
 * (new tabs append at the end, so the neighbor is rarely the opener), and
 * picks the tab that takes a closed split pane's place.
 *
 * Module state is per-window by construction: each OS window runs its own
 * chrome webview and JS context.
 */

import type { TabId, WorkspaceId } from '@/bindings/types'

interface ActiveTabEntry {
  id: TabId
  workspaceId: WorkspaceId
}

const HISTORY_CAP = 32

/** Most recent first: the active tab, then the one before it, and so on. */
let history: ActiveTabEntry[] = []

/** Record an activation; no-op when the active tab is unchanged. */
export function noteActiveTab(id: TabId | null, workspaceId: WorkspaceId | null): void {
  if (!id || !workspaceId) return
  // A refresh too when the same tab moved to another workspace, so the
  // same-workspace guard in closeReturnTarget compares live data.
  history = [{ id, workspaceId }, ...history.filter((e) => e.id !== id)].slice(0, HISTORY_CAP)
}

/**
 * Drop entries whose tabs no longer exist (closes that never went through
 * closeTabAnimated: workspace deletion, bulk closes, other windows).
 */
export function pruneTabMru(isLive: (id: TabId) => boolean): void {
  history = history.filter((e) => isLive(e.id))
}

/**
 * Drop a closing tab from the history so it can never become a return
 * target after it is gone.
 */
export function noteTabClosed(id: TabId): void {
  history = history.filter((e) => e.id !== id)
}

/** The tab active before the current one (Switch-to-last-tab target). */
export function previousTab(): TabId | null {
  return history[1]?.id ?? null
}

/** This window's tabs, most recently active first. */
export function recentTabs(): TabId[] {
  return history.map((e) => e.id)
}

/**
 * The tab to activate when closing tab `id`: the previously used tab, but
 * only when `id` is the currently active tab and the previous one lives in
 * the same workspace (a cross-workspace return would drag the window to
 * another workspace). Null means the caller should let core pick the
 * tab_order neighbor as before.
 */
export function closeReturnTarget(id: TabId): TabId | null {
  const [current, previous] = history
  if (current?.id !== id) return null
  if (!previous || previous.id === id) return null
  if (previous.workspaceId !== current.workspaceId) return null
  return previous.id
}
