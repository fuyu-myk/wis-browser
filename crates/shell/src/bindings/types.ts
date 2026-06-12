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
declare const TabFolderIdBrand: unique symbol
declare const DownloadIdBrand: unique symbol
declare const ThemeIdBrand: unique symbol

export type TabId = string & { readonly [TabIdBrand]: never }
export type WorkspaceId = string & { readonly [WorkspaceIdBrand]: never }
export type WidgetId = string & { readonly [WidgetIdBrand]: never }
export type TabFolderId = string & { readonly [TabFolderIdBrand]: never }
export type DownloadId = string & { readonly [DownloadIdBrand]: never }
export type ThemeId = string & { readonly [ThemeIdBrand]: never }

// ---------------------------------------------------------------------------
// Enums: serialised by serde as snake_case strings.
// ---------------------------------------------------------------------------

export type LayoutMode = 'sidebar_left' | 'sidebar_right' | 'top_tabs'

export type RailSide = 'left' | 'right'

export type SearchEngine = 'duckduckgo' | 'google' | 'bing'

export type ThemeMode = 'light' | 'dark' | 'system'

export type MotionLevel = 'full' | 'reduced' | 'off'

export type PrivacyMode = 'strict' | 'balanced' | 'permissive'

export type SplitDir = 'horizontal' | 'vertical'

/** Mirror of `wis_core::UpstreamProxy` (serde-tagged by `kind`). */
export type UpstreamProxy =
  | { kind: 'socks5'; host: string; port: number }
  | { kind: 'tor' }

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
  | 'restore_closed_tab'
  | 'next_workspace'
  | 'previous_workspace'
  | 'next_tab'
  | 'previous_tab'
  | 'hard_reload'
  | 'zoom_in'
  | 'zoom_out'
  | 'zoom_reset'
  | 'open_downloads'
  | 'toggle_bookmarks'
  | 'bookmark_page'
  | 'pin_tab'
  | 'duplicate_tab'
  | 'copy_url'
  | 'toggle_sidebar'

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
  | 'reddit'
  | 'line'
  | 'chat_gpt'
  | 'claude'
  | 'gemini'
  | 'grok'
  | 'deep_seek'
  | 'qwen'
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
  /** Containing tab folder, or null when the tab is loose (pinned tabs are loose). */
  folder_id: TabFolderId | null
  muted: boolean
  /** In-memory only; reset to false on app launch. */
  loading: boolean
  /** Page-zoom factor (1.0 = 100%); persisted per tab. */
  zoom: number
  created_at: string // ISO 8601 from chrono::DateTime<Utc>
  last_active_at: string
}

export interface EssentialPin {
  slot: number
  label: string
  url: string
  icon: string | null
}

/** A named, collapsible grouping of live tabs within a workspace. */
export interface TabFolder {
  id: TabFolderId
  name: string
  accent: AccentColor
  /** Collapsed folders hide their tabs in the list and let those tabs sleep. */
  collapsed: boolean
  /** Order among a workspace's folders (ascending). */
  position: number
  /** Encoded icon reference (`glyph:`/`glyph-fill:`/`emoji:`); null = colour pill. */
  icon: string | null
}

export type ImageFit = 'cover' | 'contain' | 'tile'

export interface GradientStop {
  /** `#rrggbb` hex colour. */
  color: string
  /** Percent along the gradient line, 0..=100. */
  position: number
}

export interface GradientSpec {
  /** Degrees, 0..=180 (CSS linear-gradient angle). */
  angle: number
  /** 2..=8 stops, positions ascending. */
  stops: GradientStop[]
  /** Whole-gradient opacity percent, 0..=100. */
  opacity: number
}

export interface ImageBackground {
  /** sha256 hex of the imported wallpaper. */
  asset: string
  fit: ImageFit
  dim: number
  blur: number
}

/** A workspace's custom background (externally tagged serde enum). */
export type Background = { gradient: GradientSpec } | { image: ImageBackground }

/** One mode's editable theme tokens (all hex colours). */
export interface ModeTokens {
  accent: string
  bg: string
  surface: string
  surface_raised: string
  chrome_bg: string
  content_bg: string
  text_primary: string
  text_secondary: string
  border: string
}

export interface ThemeTokens {
  light: ModeTokens
  dark: ModeTokens
}

/** A user-built chrome theme. */
export interface CustomTheme {
  id: ThemeId
  name: string
  tokens: ThemeTokens
}

/** Per-mode background memory: what each mode last looked like. */
export interface BackgroundDrafts {
  gradient: GradientSpec | null
  image: ImageBackground | null
}

export interface Workspace {
  id: WorkspaceId
  name: string
  accent: AccentColor
  icon: string | null
  tab_order: TabId[]
  pinned_tab_ids: TabId[]
  /** Live-tab folders; membership is on the tab (`Tab.folder_id`). */
  tab_folders: TabFolder[]
  /** Folder new tabs file into; null = follow the active tab. Session-only. */
  selected_folder_id: TabFolderId | null
  essentials: EssentialPin[]
  active_tab_id: TabId | null
  privacy_mode: PrivacyMode
  /** In-window split layout; null = single pane (active tab). */
  pane_tree: PaneNode | null
  /** Private/incognito workspace: non-persistent data store, never saved. */
  ephemeral: boolean
  /** Upstream proxy this workspace routes through (Tor/SOCKS5/HTTP); null = direct. */
  upstream_proxy: UpstreamProxy | null
  /** Custom page background; null = theme default. */
  background: Background | null
  /** Custom flat dot colour overriding the accent preset; null = preset. */
  custom_accent: string | null
  /** Per-mode background memory; restored when switching modes. */
  background_drafts: BackgroundDrafts
}

/**
 * User agent a remote page is shown. Mirrors `wis_core::UaProfile`
 * (externally tagged). `'safari'` is the default and fixes sites that flag the
 * raw WKWebView engine as an unsupported browser.
 */
export type UaProfile = 'default' | 'safari' | 'chrome' | { custom: string }

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
  /** Discard background tab webviews after they idle. */
  tab_sleep_enabled: boolean
  /** Idle minutes before a hidden tab's webview is discarded. */
  tab_sleep_minutes: number
  /** Persist (encrypted) login sessions across restarts; auto-expire after a cap. */
  persist_sessions: boolean
  /** WebKit fraudulent-website warning (Apple-proxied Safe Browsing). */
  fraud_warning_enabled: boolean
  /** Default user agent presented to remote pages. */
  user_agent_profile: UaProfile
  /** Per-host user agent overrides, keyed by `www.`-stripped host. */
  ua_site_overrides: Record<string, UaProfile>
  /** Global quick-launch sidebar widgets, in display order. */
  sidebar_widgets: Widget[]
  /** Cap loaded widget panels (LRU reclaim); off means limitless. */
  widget_panel_limit_enabled: boolean
  /** Loaded widget panels kept before the oldest hidden one is reclaimed. */
  widget_panel_limit: number
  /** False = dismissing a widget panel destroys its webview immediately. */
  widget_keep_loaded: boolean
  /** Minutes a hidden widget panel stays loaded before reclaim. */
  widget_sleep_minutes: number
  /** User-supplied accelerator overrides, sparse. Keyed by `KeyAction`. */
  keybindings: Partial<Record<KeyAction, string>>
  /** Ask where to save each download via a native save panel. */
  download_ask_where: boolean
  /** Directory of the last panel-chosen destination; the panel starts here. */
  download_last_dir: string | null
  /** Saved custom workspace colours (extra swatches in the recolor grid). */
  custom_accent_palette: string[]
  /** User-built chrome themes. */
  custom_themes: CustomTheme[]
  /** Active custom theme; null = built-in accent presets. */
  active_theme: ThemeId | null
  /** Chrome animation level. */
  motion: MotionLevel
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
  tab_sleep_enabled?: boolean
  tab_sleep_minutes?: number
  persist_sessions?: boolean
  fraud_warning_enabled?: boolean
  user_agent_profile?: UaProfile
  /** Replaces the whole override map when present. */
  ua_site_overrides?: Record<string, UaProfile>
  /** Replaces the whole widget list when present. */
  sidebar_widgets?: Widget[]
  widget_panel_limit_enabled?: boolean
  widget_panel_limit?: number
  widget_keep_loaded?: boolean
  widget_sleep_minutes?: number
  /**
   * Sparse map of accelerator overrides. An entry replaces just that
   * action's binding; an empty string acts as "reset to default".
   * `undefined` means "no change to keybindings".
   */
  keybindings?: Partial<Record<KeyAction, string>>
  download_ask_where?: boolean
  download_last_dir?: string
  /** Replaces the whole palette when present. */
  custom_accent_palette?: string[]
  /** Replaces the whole theme list when present. */
  custom_themes?: CustomTheme[]
  motion?: MotionLevel
}

// ---------------------------------------------------------------------------
// Peek link-safety (mirror of `wis_core::link_safety`).
// ---------------------------------------------------------------------------

export type RiskLevel = 'low' | 'medium' | 'high'

export interface LinkWarning {
  code: string
  message: string
}

export interface LinkAssessment {
  risk: RiskLevel
  warnings: LinkWarning[]
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
// Downloads (mirror of `downloads.rs` `DownloadItem`, camelCase serde).
// ---------------------------------------------------------------------------

export type DownloadStatus = 'in_progress' | 'completed' | 'failed' | 'cancelled'

/** One webview's WebContent process footprint (task manager readout). */
export interface WebviewMemory {
  label: string
  pid: number
  bytes: number
}

/** One tracked download (this session's, or a persisted completed one). */
export interface DownloadItem {
  id: DownloadId
  url: string
  filename: string
  path: string
  receivedBytes: number
  status: DownloadStatus
  /** From a private context; saved to disk but never in persistent history. */
  private: boolean
  startedAtMs: number
}

// ---------------------------------------------------------------------------
// Bookmarks (mirror of `wis_storage::bookmarks`).
// ---------------------------------------------------------------------------

export interface BookmarkFolder {
  id: number
  workspace_id: WorkspaceId
  name: string
  position: number
  created_at: string
}

export interface Bookmark {
  id: number
  workspace_id: WorkspaceId
  /** Containing folder, or null when unfiled. */
  folder_id: number | null
  url: string
  title: string
  favicon_hash: string | null
  position: number
  /** Pinned to the quick-access row, independent of `folder_id`. */
  pinned: boolean
  /** Order within the pinned row (separate from `position`). */
  pin_position: number
  created_at: string
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
