// TypeScript mirror of the Rust types in `crates/core/src/`.
// Future plan: generate by specta/tauri-specta once the type surface
// stabilises.
//
// Field names are kept exactly as serde emits them (snake_case
// on enum variants, original field names on structs). Wrapper functions
// in `./commands.ts` accept camelCase arguments and translate at the
// boundary.

// ---------------------------------------------------------------------------
// Newtype IDs: branded strings so the type system distinguishes them.
// ---------------------------------------------------------------------------

declare const TabIdBrand: unique symbol
declare const WorkspaceIdBrand: unique symbol
declare const WidgetIdBrand: unique symbol

export type TabId = string & { readonly [TabIdBrand]: never }
export type WorkspaceId = string & { readonly [WorkspaceIdBrand]: never }
export type WidgetId = string & { readonly [WidgetIdBrand]: never }

// ---------------------------------------------------------------------------
// Enums: serialised by serde as snake_case strings.
// ---------------------------------------------------------------------------

export type LayoutMode = 'sidebar_left' | 'sidebar_right' | 'top_tabs'

export type RailSide = 'left' | 'right'

export type SearchEngine = 'duckduckgo' | 'google' | 'bing'

export type ThemeMode = 'light' | 'dark' | 'system'

export type PrivacyMode = 'strict' | 'balanced' | 'permissive'

export type SplitDir = 'horizontal' | 'vertical'

export type PaneBranch = 'first' | 'second'

/**
 * Mirror of `wis_core::PaneNode` (serde-tagged by `kind`). `ratio` is permille
 * (0..1000) of the space given to `first`.
 */
export type PaneNode =
  | { kind: 'leaf'; tab: TabId }
  | { kind: 'split'; dir: SplitDir; ratio: number; first: PaneNode; second: PaneNode }

/**
 * Mirror of `wis_core::KeyAction`. Wire format is snake_case.
 * Two variants per "alternate" action so users can rebind primary
 * and alt independently in the configurable-shortcuts pass.
 */
export type KeyAction =
  | 'new_tab'
  | 'new_workspace'
  | 'close_tab'
  | 'focus_url_bar'
  | 'focus_url_bar_alt'
  | 'reload'
  | 'go_back'
  | 'go_back_alt'
  | 'go_forward'
  | 'go_forward_alt'
  | 'switch_to_tab1'
  | 'switch_to_tab2'
  | 'switch_to_tab3'
  | 'switch_to_tab4'
  | 'switch_to_tab5'
  | 'switch_to_tab6'
  | 'switch_to_tab7'
  | 'switch_to_tab8'
  | 'switch_to_tab9'
  | 'switch_to_last_tab'
  | 'find_in_page'
  | 'find_next'
  | 'find_previous'
  | 'open_command_palette'
  | 'open_settings'
  | 'open_history'
  | 'clear_browsing_data'
  | 'new_private_window'
  | 'new_window'
  | 'split_pane_horizontal'
  | 'split_pane_vertical'
  | 'close_pane'
  | 'focus_next_pane'
  | 'focus_pane_left'
  | 'focus_pane_right'
  | 'focus_pane_up'
  | 'focus_pane_down'
  | 'unsplit_panes'

export type AccentColor =
  | 'primary'
  | 'mint'
  | 'rose'
  | 'sun'
  | 'lila'
  | 'gray'
  | 'ink'
  | 'azur'

export type WidgetKind =
  | 'whats_app'
  | 'telegram'
  | 'discord'
  | 'slack'
  | 'messenger'
  | 'x_twitter'
  | 'instagram'
  | 'custom'

// ---------------------------------------------------------------------------
// Domain types.
// ---------------------------------------------------------------------------

export interface FaviconRef {
  hash: string
}

export interface Tab {
  id: TabId
  workspace_id: WorkspaceId
  url: string // serialised from url::Url
  title: string
  favicon: FaviconRef | null
  pinned: boolean
  muted: boolean
  /** In-memory only; reset to false on app launch. */
  loading: boolean
  created_at: string // ISO 8601 from chrono::DateTime<Utc>
  last_active_at: string
}

export interface EssentialPin {
  slot: number
  label: string
  url: string
  icon: string | null
}

export interface Workspace {
  id: WorkspaceId
  name: string
  accent: AccentColor
  icon: string | null
  tab_order: TabId[]
  pinned_tab_ids: TabId[]
  essentials: EssentialPin[]
  active_tab_id: TabId | null
  privacy_mode: PrivacyMode
  /** In-window split layout; null = single pane (active tab). */
  pane_tree: PaneNode | null
  /** Private/incognito workspace: non-persistent data store, never saved. */
  ephemeral: boolean
}

export interface UserSettings {
  layout: LayoutMode
  top_tabs_rail_side: RailSide
  theme: ThemeMode
  accent: AccentColor
  glass_enabled: boolean
  sidebar_collapsible: boolean
  compact: boolean
  default_privacy_mode: PrivacyMode
  doh_resolver: string | null
  homepage: string | null
  search_engine: SearchEngine
  filter_list_auto_update: boolean
  url_bar_focus_glow: boolean
  /** Launch directly into a private (ephemeral) workspace. */
  always_private: boolean
  /** User-supplied accelerator overrides, sparse. Keyed by `KeyAction`. */
  keybindings: Partial<Record<KeyAction, string>>
}

/**
 * Partial update for `UserSettings`. Fields left undefined mean "no change".
 * Setting a field replaces the corresponding `UserSettings` field.
 */
export interface UserSettingsPatch {
  layout?: LayoutMode
  top_tabs_rail_side?: RailSide
  theme?: ThemeMode
  accent?: AccentColor
  glass_enabled?: boolean
  sidebar_collapsible?: boolean
  compact?: boolean
  default_privacy_mode?: PrivacyMode
  doh_resolver?: string
  homepage?: string
  search_engine?: SearchEngine
  filter_list_auto_update?: boolean
  url_bar_focus_glow?: boolean
  always_private?: boolean
  /**
   * Sparse map of accelerator overrides. An entry replaces just that
   * action's binding; an empty string acts as "reset to default".
   * `undefined` means "no change to keybindings".
   */
  keybindings?: Partial<Record<KeyAction, string>>
}

export interface PerSitePolicy {
  host: string
  allow_javascript: boolean
  allow_cookies: boolean
  allow_third_party: boolean
  allow_camera: boolean
  allow_microphone: boolean
  allow_location: boolean
}

export interface Widget {
  id: WidgetId
  kind: WidgetKind
  label: string
  url: string
  icon: string | null
  enabled: boolean
}

export interface BrowserState {
  workspaces: Workspace[]
  tabs: Tab[]
  active_workspace_id: WorkspaceId
  settings: UserSettings
}

// ---------------------------------------------------------------------------
// Errors: tagged union matching `WisError`'s serde representation.
// ---------------------------------------------------------------------------

export type WisError =
  | { code: 'workspace_not_found'; id: WorkspaceId }
  | { code: 'tab_not_found'; id: TabId }
  | { code: 'invalid_url'; url: string }
  | { code: 'cannot_delete_last_workspace' }
  | { code: 'tab_workspace_mismatch'; tab_id: TabId; workspace_id: WorkspaceId }
  | { code: 'invalid_input'; message: string }
  | { code: 'persistence_failed'; reason: string }

// ---------------------------------------------------------------------------
// Privacy decision engine (wis-network).
// ---------------------------------------------------------------------------

export type RequestType =
  | 'document'
  | 'subdocument'
  | 'stylesheet'
  | 'script'
  | 'image'
  | 'font'
  | 'media'
  | 'xhr'
  | 'websocket'
  | 'other'

export type Decision =
  | { kind: 'allow' }
  | { kind: 'rewrite'; url: string }
  | { kind: 'block'; reason: string }

// ---------------------------------------------------------------------------
// Browsing history (mirror of `wis_storage::history`).
// ---------------------------------------------------------------------------

/** One deduplicated history row (one per URL per workspace). */
export interface HistoryEntry {
  id: number
  workspace_id: WorkspaceId
  url: string
  title: string
  visited_at: string // ISO 8601 from chrono::DateTime<Utc>
  visit_count: number
}

/** A domain with stored site data (cookies/cache/storage) in a workspace. */
export interface SiteDataEntry {
  domain: string
}

// ---------------------------------------------------------------------------
// Password vault (mirror of `vault.rs`).
// ---------------------------------------------------------------------------

/** Vault lock state + whether it's been set up / supported on this platform. */
export interface VaultStatus {
  unlocked: boolean
  has_master_key: boolean
  supported: boolean
}

/** A saved credential without its password (safe to list). */
export interface CredentialSummary {
  id: number
  host: string
  username: string
  updated_at: string
}

// ---------------------------------------------------------------------------
// URL-bar suggestions (mirror of `suggest.rs`).
// ---------------------------------------------------------------------------

export interface SearchSuggestion {
  query: string
  count: number
}

export interface SiteSuggestion {
  id: number
  url: string
  title: string
}

export interface UrlSuggestions {
  searches: SearchSuggestion[]
  sites: SiteSuggestion[]
}

/** Query for `getHistory`: substring filter (empty = all), newest-first. */
export interface HistoryQuery {
  search: string
  /** Restrict to one workspace; null returns all workspaces. */
  workspace_id: WorkspaceId | null
  limit: number
  offset: number
}

export function isWisError(value: unknown): value is WisError {
  return (
    typeof value === 'object' &&
    value !== null &&
    'code' in value &&
    typeof (value as { code: unknown }).code === 'string'
  )
}
