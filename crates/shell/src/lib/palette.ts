/**
 * Command palette domain layer.
 *
 * A palette source maps a query to a ranked list of items. The panel
 * renders sections in a fixed order; each section sorts internally by
 * score.
 *
 * Sources are synchronous, so the two that need stored data (bookmarks and
 * history) take it as an argument.
 */

import type { ReactNode } from 'react'

import type {
  AccentColor,
  Bookmark,
  ClosedTabRecord,
  BrowserState,
  HistoryEntry,
  KeyAction,
  Tab,
  UserSettings,
  UserSettingsPatch,
} from '@/bindings/types'
import { localizeAccelerator, prettyAccelerator } from '@/lib/keybindings'
import { resolveInput, type SearchSettings } from '@/lib/url'

export type PaletteSection =
  | 'tabs'
  | 'closed'
  | 'bookmarks'
  | 'history'
  | 'actions'
  | 'settings'
  | 'open'

// Close-tab-from-palette secondary action: rendered per platform (⌘⌫ on
// macOS, Ctrl+Backspace elsewhere), matching its Cmd/Ctrl+Backspace handler.
const CLOSE_TAB_SHORTCUT = prettyAccelerator(localizeAccelerator('Cmd+Backspace'))

export interface TabMeta {
  workspaceName: string
  workspaceAccent: AccentColor
  /** True when the tab belongs to a workspace other than the active one. */
  crossWorkspace: boolean
}

export interface SecondaryAction {
  /** Short label shown in tooltip / footer hint, e.g. "Close tab". */
  label: string
  /** Pretty shortcut string shown on the row, e.g. "⌘⌫". */
  shortcut: string
  /** Invoked when the user triggers the shortcut from the highlighted row. */
  run: () => void | Promise<void>
}

export interface PaletteItem {
  id: string
  section: PaletteSection
  score: number
  title: string
  subtitle?: string
  accessory?: ReactNode
  icon?: ReactNode
  /** Indices into `title` that matched the query; used for highlighting. */
  titleMatches?: number[]
  /** Indices into `subtitle` that matched the query. */
  subtitleMatches?: number[]
  /**
   * Tab-only metadata.
   */
  tabMeta?: TabMeta
  /**
   * Optional companion action invoked by a row-scoped shortcut (e.g.
   * Cmd+Backspace closes a tab from the palette without dismissing it).
   */
  secondaryAction?: SecondaryAction
  activate: () => void | Promise<void>
}

export type PaletteSource = (query: string) => PaletteItem[]

/**
 * Overlay user-supplied accelerator overrides on top of `DEFAULT_KEYMAP`.
 * Mirrors `wis_core::effective_keymap` on the Rust side, so the palette
 * and any UI that wants to render the current accelerator pill stays in
 * sync with the native menu.
 *
 * `overrides` is typically `settings.keybindings`. An entry whose value
 * is the empty string is treated as "no binding".
 */
export function effectiveKeymap(
  overrides: Partial<Record<KeyAction, string>> | undefined,
): Partial<Record<KeyAction, string>> {
  if (!overrides) return DEFAULT_KEYMAP
  const merged: Partial<Record<KeyAction, string>> = { ...DEFAULT_KEYMAP }
  for (const [action, accel] of Object.entries(overrides) as Array<
    [KeyAction, string]
  >) {
    if (accel === '') {
      delete merged[action]
    } else {
      merged[action] = accel
    }
  }
  return merged
}

// Canonical (macOS) default bindings, mirroring `default_keymap_for(true)` in
// crates/core/src/keybinding.rs. Localized to the host platform just below.
const DEFAULT_KEYMAP_MAC: Partial<Record<KeyAction, string>> = {
  new_tab: 'Cmd+T',
  new_window: 'Cmd+N',
  new_workspace: 'Alt+Cmd+N',
  close_tab: 'Cmd+W',
  focus_url_bar: 'Cmd+E',
  reload: 'Cmd+R',
  go_back: 'Cmd+[',
  go_back_alt: 'Cmd+Left',
  go_forward: 'Cmd+]',
  go_forward_alt: 'Cmd+Right',
  switch_to_tab1: 'Cmd+1',
  switch_to_tab2: 'Cmd+2',
  switch_to_tab3: 'Cmd+3',
  switch_to_tab4: 'Cmd+4',
  switch_to_tab5: 'Cmd+5',
  switch_to_tab6: 'Cmd+6',
  switch_to_tab7: 'Cmd+7',
  switch_to_tab8: 'Cmd+8',
  switch_to_last_tab: 'Cmd+9',
  find_in_page: 'Cmd+F',
  find_next: 'Cmd+G',
  find_previous: 'Cmd+Shift+G',
  open_command_palette: 'Cmd+L',
  open_settings: 'Cmd+,',
  open_history: 'Cmd+Shift+H',
  clear_browsing_data: 'Cmd+Shift+Backspace',
  toggle_private_workspace: 'Cmd+Shift+N',
  split_pane_horizontal: 'Cmd+D',
  split_pane_vertical: 'Cmd+Shift+D',
  close_pane: 'Alt+Cmd+W',
  focus_pane_left: 'Alt+Cmd+Left',
  focus_pane_right: 'Alt+Cmd+Right',
  focus_pane_up: 'Alt+Cmd+Up',
  focus_pane_down: 'Alt+Cmd+Down',
  unsplit_panes: 'Alt+Cmd+U',
  restore_closed_tab: 'Cmd+Shift+T',
  next_workspace: 'Alt+Cmd+E',
  previous_workspace: 'Alt+Cmd+Q',
  next_tab: 'Cmd+Shift+]',
  previous_tab: 'Cmd+Shift+[',
  hard_reload: 'Cmd+Shift+R',
  toggle_reader: 'Alt+Cmd+R',
  zoom_in: 'Cmd+=',
  zoom_out: 'Cmd+-',
  zoom_reset: 'Cmd+0',
  open_downloads: 'Cmd+J',
  toggle_bookmarks: 'Alt+Cmd+B',
  bookmark_page: 'Cmd+Shift+B',
  pin_tab: 'Cmd+Shift+P',
  duplicate_tab: 'Cmd+Shift+K',
  copy_url: 'Cmd+Shift+C',
  print: 'Cmd+P',
  toggle_sidebar: 'Cmd+S',
  element_picker: 'Cmd+Shift+X',
  pop_out_video: 'Cmd+Shift+O',
  inspect: 'Alt+Cmd+I',
}

// Host-platform defaults: Windows/Linux swap `Cmd` for `Ctrl` (mirrors
// `default_keymap_for` in keybinding.rs); macOS passes through unchanged.
export const DEFAULT_KEYMAP: Partial<Record<KeyAction, string>> =
  Object.fromEntries(
    Object.entries(DEFAULT_KEYMAP_MAC).map(([action, accel]) => [
      action,
      localizeAccelerator(accel),
    ]),
  ) as Partial<Record<KeyAction, string>>

const ACTION_LABELS: Record<KeyAction, string> = {
  new_tab: 'New tab',
  new_workspace: 'New workspace',
  close_tab: 'Close tab',
  focus_url_bar: 'Focus address bar',
  focus_url_bar_alt: 'Focus address bar (alt)',
  reload: 'Reload tab',
  go_back: 'Back',
  go_back_alt: 'Back (alt)',
  go_forward: 'Forward',
  go_forward_alt: 'Forward (alt)',
  switch_to_tab1: 'Switch to tab 1',
  switch_to_tab2: 'Switch to tab 2',
  switch_to_tab3: 'Switch to tab 3',
  switch_to_tab4: 'Switch to tab 4',
  switch_to_tab5: 'Switch to tab 5',
  switch_to_tab6: 'Switch to tab 6',
  switch_to_tab7: 'Switch to tab 7',
  switch_to_tab8: 'Switch to tab 8',
  switch_to_tab9: 'Switch to tab 9',
  switch_to_last_tab: 'Switch to last tab',
  find_in_page: 'Find in page',
  find_next: 'Find next match',
  find_previous: 'Find previous match',
  open_command_palette: 'Open command palette',
  open_settings: 'Open settings',
  open_history: 'Show history',
  clear_browsing_data: 'Clear browsing data',
  new_private_window: 'New private window',
  toggle_private_workspace: 'Toggle private workspace',
  new_window: 'New window',
  split_pane_horizontal: 'Split pane right',
  split_pane_vertical: 'Split pane down',
  close_pane: 'Close pane',
  focus_next_pane: 'Focus next pane',
  focus_pane_left: 'Focus pane left',
  focus_pane_right: 'Focus pane right',
  focus_pane_up: 'Focus pane up',
  focus_pane_down: 'Focus pane down',
  unsplit_panes: 'Unsplit panes',
  restore_closed_tab: 'Reopen closed tab',
  next_workspace: 'Next workspace',
  previous_workspace: 'Previous workspace',
  next_tab: 'Next tab',
  previous_tab: 'Previous tab',
  hard_reload: 'Reload (bypass cache)',
  toggle_reader: 'Toggle reader mode',
  zoom_in: 'Zoom in',
  zoom_out: 'Zoom out',
  zoom_reset: 'Reset zoom',
  open_downloads: 'Show downloads',
  toggle_bookmarks: 'Show bookmarks',
  bookmark_page: 'Bookmark this page',
  pin_tab: 'Pin or unpin tab',
  duplicate_tab: 'Duplicate tab',
  copy_url: 'Copy current URL',
  print: 'Print page',
  toggle_sidebar: 'Toggle floating sidebar',
  element_picker: 'Hide element (picker)',
  pop_out_video: 'Pop out video',
  inspect: 'Inspect',
  capture_visible: 'Capture screenshot',
  capture_region: 'Capture selected region',
  capture_window: 'Capture window',
}

const ALL_ACTIONS = Object.keys(ACTION_LABELS) as KeyAction[]

/** Stable ordered list of every bindable action. Used by the
 *  Settings page's keyboard-shortcuts editor. */
export function allActions(): readonly KeyAction[] {
  return ALL_ACTIONS
}

export function actionLabel(action: KeyAction): string {
  return ACTION_LABELS[action]
}

export interface FuzzyMatch {
  score: number
  /** Indices into `haystack` where each query char matched. Empty on miss. */
  indices: number[]
}

/**
 * Subsequence-aware fuzzy match. Returns each query char's position in
 * the haystack so the renderer can highlight them.
 *
 * Score combines three signals:
 *   - earlier start (later starts get a lower bonus),
 *   - contiguous matches (a tight cluster outranks scattered chars),
 *   - exact-substring presence (substring matches dominate scattered).
 *
 * Eg: "youtube" + "out" beats "youtube" + "ye" (contiguous substring
 * vs scattered), and "DuckDuckGo" + "ddg" still scores positively while
 * staying behind a contiguous match against "DuckDuckGo" + "duck".
 */
export function fuzzyMatch(haystack: string, query: string): FuzzyMatch {
  if (!query) return { score: 1, indices: [] }
  if (!haystack) return { score: 0, indices: [] }
  const h = haystack.toLowerCase()
  const q = query.toLowerCase()

  let qi = 0
  const indices: number[] = []
  for (let i = 0; i < h.length && qi < q.length; i++) {
    if (h[i] === q[qi]) {
      indices.push(i)
      qi++
    }
  }
  if (qi < q.length) return { score: 0, indices: [] }

  const startBonus = Math.max(0, 50 - indices[0])
  const span = indices[indices.length - 1] - indices[0] + 1
  const contiguityBonus = Math.floor((q.length / span) * 50)
  const substringBonus = h.includes(q) ? 50 : 0
  const score = Math.max(1, startBonus + contiguityBonus + substringBonus)
  return { score, indices }
}

interface TabsSourceArgs {
  state: BrowserState
  onActivate: (tab: Tab) => void | Promise<void>
  onClose: (tab: Tab) => void | Promise<void>
}

export function makeTabsSource({ state, onActivate, onClose }: TabsSourceArgs): PaletteSource {
  const workspaceByTab = new Map<string, { name: string; accent: AccentColor; id: string }>()
  for (const ws of state.workspaces) {
    for (const tabId of ws.tab_order) {
      workspaceByTab.set(tabId, { name: ws.name, accent: ws.accent, id: ws.id })
    }
  }

  return (query) => {
    const items = state.tabs.map((tab) => {
      const title = tab.title && tab.title !== tab.url ? tab.title : tab.url
      const ws = workspaceByTab.get(tab.id)
      const titleMatch = fuzzyMatch(title, query)
      const urlMatch = fuzzyMatch(tab.url, query)
      const workspaceMatch = ws ? fuzzyMatch(ws.name, query) : { score: 0, indices: [] }
      const score = Math.max(titleMatch.score, urlMatch.score, workspaceMatch.score)
      const item: PaletteItem = {
        id: `tab:${tab.id}`,
        section: 'tabs',
        score,
        title,
        subtitle: tab.url,
        titleMatches: titleMatch.score > 0 ? titleMatch.indices : undefined,
        subtitleMatches: urlMatch.score > 0 ? urlMatch.indices : undefined,
        tabMeta: ws
          ? {
              workspaceName: ws.name,
              workspaceAccent: ws.accent,
              crossWorkspace: ws.id !== state.active_workspace_id,
            }
          : undefined,
        secondaryAction: {
          label: 'Close tab',
          shortcut: CLOSE_TAB_SHORTCUT,
          run: () => onClose(tab),
        },
        activate: () => onActivate(tab),
      }
      return item
    })
    return items.filter((it) => it.score > 0).sort((a, b) => b.score - a.score)
  }
}

interface ActionsSourceArgs {
  keymap: Partial<Record<KeyAction, string>>
  onDispatch: (action: KeyAction) => void | Promise<void>
}

/**
 * Positional switch-to-tab actions (Cmd+1..8/9) are excluded from the palette:
 * nine near-identical "Switch to tab N" rows flood the Actions section on any
 * "tab"/"switch" query, and the Tabs section already switches to any tab by
 * name. The shortcuts stay discoverable in Settings -> Shortcuts.
 *
 * Platform-unsupported actions (e.g. some capture modes off macOS) stay listed
 * and, on dispatch, raise a "not available on {target}" toast (see `platformSupport`).
 */
const PALETTE_HIDDEN_ACTIONS: ReadonlySet<KeyAction> = new Set<KeyAction>([
  'switch_to_tab1',
  'switch_to_tab2',
  'switch_to_tab3',
  'switch_to_tab4',
  'switch_to_tab5',
  'switch_to_tab6',
  'switch_to_tab7',
  'switch_to_tab8',
  'switch_to_tab9',
])

const PALETTE_ACTIONS = ALL_ACTIONS.filter((a) => !PALETTE_HIDDEN_ACTIONS.has(a))

export function makeActionsSource({ keymap, onDispatch }: ActionsSourceArgs): PaletteSource {
  return (query) => {
    const items = PALETTE_ACTIONS.map((action) => {
      const label = actionLabel(action)
      const match = fuzzyMatch(label, query)
      const item: PaletteItem = {
        id: `action:${action}`,
        section: 'actions',
        score: match.score,
        title: label,
        titleMatches: match.score > 0 ? match.indices : undefined,
        accessory: keymap[action] ?? '—',
        activate: () => onDispatch(action),
      }
      return item
    })
    return items.filter((it) => it.score > 0).sort((a, b) => b.score - a.score)
  }
}

interface NavigateSourceArgs {
  onOpen: (url: string) => void | Promise<void>
  /** Search-resolution slice of the user settings (engine + custom engines). */
  settings: SearchSettings
}

/**
 * Type-to-navigate fallback. Whenever the query is non-empty, emit a
 * single row that resolves the query through `resolveInput` (keyword
 * triggers included) and opens it as a tab. Lets the palette act as a
 * launcher when nothing matches (and as a quick "open this" when something
 * does). Always sorts last because of the dedicated 'open' section in the
 * surface.
 */
export function makeNavigateSource({ onOpen, settings }: NavigateSourceArgs): PaletteSource {
  return (query) => {
    const resolved = resolveInput(query, settings)
    if (!resolved) return []
    const trimmed = query.trim()
    const title = resolved.isSearch ? `Search for "${trimmed}"` : `Go to ${resolved.url}`
    const subtitle = resolved.isSearch && resolved.engineLabel ? `via ${resolved.engineLabel}` : undefined
    return [
      {
        id: 'open:current',
        section: 'open',
        score: 1,
        title,
        subtitle,
        activate: () => onOpen(resolved.url),
      },
    ]
  }
}

/** Rows either page section will contribute. */
const PAGE_LIMIT = 8
/** Visits past this stop counting. */
const VISIT_CAP = 10

/**
 * Storage strips the fragment before saving a visit, so a bookmark with an
 * anchor still has to match its own history row. Mirrors `history_key`.
 */
export function historyKey(url: string): string {
  const hash = url.indexOf('#')
  return hash === -1 ? url : url.slice(0, hash)
}

/** How many recently closed tabs the palette lists with no query typed. */
const CLOSED_BROWSE_LIMIT = 5

/** A stored page a page source can rank. */
interface RankablePage {
  id: string
  url: string
  title: string
  /** Times visited; history uses it to favour pages the user returns to. */
  visits: number
  open: () => void | Promise<void>
}

/**
 * Rank stored pages against the query, matching title and URL separately so
 * either can be highlighted. Falls back to the URL as the title.
 */
function rankPages(
  pages: readonly RankablePage[],
  query: string,
  section: PaletteSection,
): PaletteItem[] {
  const items: PaletteItem[] = []
  for (const page of pages) {
    const title = page.title.trim() === '' ? page.url : page.title
    const byTitle = fuzzyMatch(title, query)
    const byUrl = fuzzyMatch(page.url, query)
    const best = Math.max(byTitle.score, byUrl.score)
    if (best === 0) continue
    // Frequency lifts a page by at most half its own match score.
    const frequency = Math.round((best * Math.min(page.visits, VISIT_CAP)) / (VISIT_CAP * 2))
    items.push({
      id: page.id,
      section,
      score: best + frequency,
      title,
      subtitle: page.url,
      titleMatches: byTitle.score > 0 ? byTitle.indices : undefined,
      subtitleMatches: byUrl.score > 0 ? byUrl.indices : undefined,
      activate: page.open,
    })
  }
  return items.sort((a, b) => b.score - a.score).slice(0, PAGE_LIMIT)
}

interface BookmarksSourceArgs {
  bookmarks: readonly Bookmark[]
  onOpen: (url: string) => void | Promise<void>
}

/**
 * Saved pages. Bookmarks are a bounded set, so the whole list is matched here
 * rather than queried per keystroke. An empty query yields nothing.
 */
export function makeBookmarksSource({ bookmarks, onOpen }: BookmarksSourceArgs): PaletteSource {
  return (query) => {
    const trimmed = query.trim()
    if (trimmed === '') return []
    const pages = bookmarks.map((bookmark) => ({
      id: `bookmark:${bookmark.id}`,
      url: bookmark.url,
      title: bookmark.title,
      visits: 0,
      open: () => onOpen(bookmark.url),
    }))
    return rankPages(pages, trimmed, 'bookmarks')
  }
}

interface HistorySourceArgs {
  entries: readonly HistoryEntry[]
  onOpen: (url: string) => void | Promise<void>
}

/**
 * Pages visited before. The ranking here orders them and marks what matched.
 * A page visited often outranks one seen once.
 */
export function makeHistorySource({ entries, onOpen }: HistorySourceArgs): PaletteSource {
  return (query) => {
    const trimmed = query.trim()
    if (trimmed === '') return []
    const pages = entries.map((entry) => ({
        id: `history:${entry.id}`,
      url: entry.url,
      title: entry.title,
      visits: entry.visit_count,
      open: () => onOpen(entry.url),
    }))
    return rankPages(pages, trimmed, 'history')
  }
}

interface ClosedTabsSourceArgs {
  entries: readonly ClosedTabRecord[]
  /** Reopens by row id, so the tab returns with the session it was closed with. */
  onReopen: (id: number) => void | Promise<void>
}

/**
 * Tabs closed recently, newest first. The palette shows the last few closed tabs
 * when no query is present. A typed query filters and ranks the whole list instead.
 */
export function makeClosedTabsSource({ entries, onReopen }: ClosedTabsSourceArgs): PaletteSource {
  return (query) => {
    const trimmed = query.trim()
    if (trimmed === '') {
      return entries.slice(0, CLOSED_BROWSE_LIMIT).map((entry, index) => ({
        id: `closed:${entry.id}`,
        section: 'closed' as const,
        // Descending.
        score: CLOSED_BROWSE_LIMIT - index,
        title: entry.title.trim() === '' ? entry.url : entry.title,
        subtitle: entry.url,
        activate: () => onReopen(entry.id),
      }))
    }
    const pages = entries.map((entry) => ({
      id: `closed:${entry.id}`,
      url: entry.url,
      title: entry.title,
      visits: 0,
      open: () => onReopen(entry.id),
    }))
    return rankPages(pages, trimmed, 'closed')
  }
}

interface SettingsTogglesSourceArgs {
  settings: UserSettings
  onChange: (patch: UserSettingsPatch) => void | Promise<void>
}

// Each entry maps a human-readable label to one boolean field on
// `UserSettings`.
interface ToggleSpec {
  key:
    | 'glass_enabled'
    | 'sidebar_collapsible'
    | 'url_bar_focus_glow'
    | 'filter_list_auto_update'
  label: string
}

const SETTINGS_TOGGLES: ToggleSpec[] = [
  { key: 'glass_enabled', label: 'Liquid glass' },
  { key: 'sidebar_collapsible', label: 'Collapsible sidebar' },
  { key: 'url_bar_focus_glow', label: 'URL bar focus glow' },
  { key: 'filter_list_auto_update', label: 'Auto-update filter lists' },
]

/**
 * Boolean-only settings toggles surfaced as palette rows.
 *
 * The accessory shows the current state ("On"/"Off"), so users see the
 * direction the activation will move the setting before they fire it.
 */
export function makeSettingsTogglesSource({
  settings,
  onChange,
}: SettingsTogglesSourceArgs): PaletteSource {
  return (query) => {
    const items = SETTINGS_TOGGLES.map((spec) => {
      const current = settings[spec.key]
      const labelMatch = fuzzyMatch(spec.label, query)
      const verbMatch = fuzzyMatch(`Toggle ${spec.label}`, query)
      const score = Math.max(labelMatch.score, verbMatch.score)
      const item: PaletteItem = {
        id: `settings:${spec.key}`,
        section: 'settings',
        score,
        title: spec.label,
        titleMatches: labelMatch.score > 0 ? labelMatch.indices : undefined,
        accessory: current ? 'On' : 'Off',
        activate: () => onChange({ [spec.key]: !current }),
      }
      return item
    })
    return items.filter((it) => it.score > 0).sort((a, b) => b.score - a.score)
  }
}
