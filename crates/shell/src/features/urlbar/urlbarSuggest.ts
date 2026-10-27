/**
 * Shared contract between the address bar (chrome "brain") and the
 * `overlay-urlbar` surface.
 *
 * The chrome owns all state: it fetches suggestions, computes inline
 * completion, assembles the flat row list (already grouped into sections via
 * each row's `section` label), and tracks the keyboard highlight. It pushes
 * `{ rows, highlight }` to the overlay via {@link URLBAR_RENDER_EVENT}; the
 * overlay reports mouse intent back via {@link URLBAR_PICK_EVENT} /
 * {@link URLBAR_REMOVE_EVENT}, each carrying the flat row index.
 */

import type { TabId } from '@/bindings/types'

/** chrome → overlay: render this list with this row highlighted. */
export const URLBAR_RENDER_EVENT = 'wis-urlbar-render'
/** overlay → chrome: the user clicked row `index` (navigate / search / switch). */
export const URLBAR_PICK_EVENT = 'wis-urlbar-pick'
/** overlay → chrome: the user clicked the ✕ on row `index` (forget it). */
export const URLBAR_REMOVE_EVENT = 'wis-urlbar-remove'

/** Section labels. Empty string = the untitled top (typed) section. */
export const SECTION_TOP = ''
export const SECTION_RECOMMENDATIONS = 'Wis Recommendations'
export const SECTION_RECENT_SEARCHES = 'Recent searches'

/**
 * One flat, renderable suggestion row. Rows are pre-ordered and carry their
 * `section` label so the overlay can insert headers on section changes.
 */
export type RenderRow =
  | {
      kind: 'site'
      section: string
      id: number
      url: string
      title: string
      /** Set when this site is already open in a tab → "switch to tab". */
      openTabId: TabId | null
    }
  | { kind: 'search'; section: string; query: string }
  | { kind: 'searchQuery'; section: string; query: string; engineLabel: string }

export interface UrlbarRenderPayload {
  rows: RenderRow[]
  /** Index of the keyboard-selected row, or -1 for "typed text". */
  highlight: number
}

export interface UrlbarIndexPayload {
  index: number
}

/** Rows the ✕ can forget (history-backed). */
export function isRemovable(row: RenderRow): boolean {
  return row.kind === 'site' || row.kind === 'search'
}

// Row metrics: kept in sync with `window.css` so the chrome can size the
// overlay webview to its content.
const ROW_H = 40
const HEAD_H = 26
const PAD_V = 6
const SECTION_GAP = 4
const MAX_H = 460

/** Pixel height of the dropdown for a given row list (headers included). */
export function estimateHeight(rows: RenderRow[]): number {
  if (rows.length === 0) return 0
  let headers = 0
  let groups = 0
  let prev: string | null = null
  for (const r of rows) {
    if (r.section !== prev) {
      groups += 1
      if (r.section) headers += 1
      prev = r.section
    }
  }
  const h = PAD_V * 2 + rows.length * ROW_H + headers * HEAD_H + Math.max(0, groups - 1) * SECTION_GAP
  return Math.min(h, MAX_H)
}

/** Host portion of a URL for the secondary line, www-stripped. */
export function hostOf(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, '')
  } catch {
    return url
  }
}

/** How the highlighted row is announced. */
export function spokenRow(row: RenderRow, index: number, count: number): string {
  const place = `${index + 1} of ${count}`
  if (row.kind === 'search') return `${row.query}, search, ${place}`
  if (row.kind === 'searchQuery') return `${row.query}, search with ${row.engineLabel}, ${place}`
  const host = hostOf(row.url)
  const parts = row.title && row.title !== host ? [row.title, host] : [host]
  if (row.openTabId) parts.push('switch to tab')
  return [...parts, place].join(', ')
}
