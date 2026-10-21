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

/** A user-defined search engine (mirror of `wis_core::CustomSearchEngine`). */
export interface CustomSearchEngine {
  name: string
  /** Address-bar trigger (`kw query`): lowercase, no whitespace, unique. */
  keyword: string
  /** Query-URL template containing `{searchTerms}`. */
  template: string
  /** At most one engine per list; wins over the built-in default. */
  is_default: boolean
}

export type ThemeMode = 'light' | 'dark' | 'system'

export type MotionLevel = 'full' | 'reduced' | 'off'

export type CornerStyle = 'round' | 'soft' | 'sharp'

export type TabShape = 'pill' | 'rounded' | 'square'

export type FolderStyle = 'pill' | 'outline' | 'text'

export type GlassIntensity = 'light' | 'medium' | 'heavy'

export type TabIndicator = 'bar' | 'underline' | 'filled' | 'dot'

export type Density = 'comfortable' | 'cozy' | 'compact'

export type CloseButton = 'always' | 'hover' | 'hidden'

/** Which glass surfaces blur when the master `glass_enabled` toggle is on. */
export interface GlassScopes {
  chrome: boolean
  menus: boolean
  tabs: boolean
}

/** Chrome geometry and material (mirror of `wis_core::design::ChromeDesign`). */
export interface ChromeDesign {
  corners: CornerStyle
  tab_shape: TabShape
  folder_style: FolderStyle
  glass_scopes: GlassScopes
  glass_intensity: GlassIntensity
  tab_indicator: TabIndicator
  density: Density
  close_button: CloseButton
  /** Sidebar width in px (clamped 180..=360 boundary-side). */
  sidebar_width: number
}

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
  | 'open_canvases'
  | 'clear_browsing_data'
  | 'new_private_window'
  | 'toggle_private_workspace'
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
  | 'toggle_reader'
  | 'zoom_in'
  | 'zoom_out'
  | 'zoom_reset'
  | 'open_downloads'
  | 'toggle_bookmarks'
  | 'bookmark_page'
  | 'pin_tab'
  | 'duplicate_tab'
  | 'copy_url'
  | 'print'
  | 'toggle_sidebar'
  | 'element_picker'
  | 'pop_out_video'
  | 'inspect'
  | 'capture_visible'
  | 'capture_region'
  | 'capture_window'

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
  | 'perplexity'
  | 'gmail'
  | 'google_calendar'
  | 'google_keep'
  | 'notion'
  | 'todoist'
  | 'outlook'
  | 'proton_mail'
  | 'proton_calendar'
  | 'custom'

/** Dock row a widget renders under (mirror of `wis_core::WidgetGroup`). */
export type WidgetGroup = 'social' | 'ai' | 'productivity' | 'custom'

// ---------------------------------------------------------------------------
// Domain types.
// ---------------------------------------------------------------------------

export interface FaviconRef {
  hash: string
}

/** Class of a page-load failure (mirror of `wis_core::LoadErrorKind`). */
export type LoadErrorKind =
  | 'dns_failure'
  | 'connection_refused'
  | 'timeout'
  | 'tls_error'
  | 'offline'
  | 'unknown'

/** Why a TLS certificate was rejected (mirror of `wis_core::TlsFault`). */
export type TlsFault = 'expired' | 'name_mismatch' | 'untrusted' | 'revoked' | 'protocol'

/** A failed page load (mirror of `wis_core::LoadError`). */
export interface LoadError {
  kind: LoadErrorKind
  /** The engine's native error code (diagnostics only). */
  code: number
  url: string
  /** The certificate fault behind a `tls_error`, `null` otherwise. */
  tls: TlsFault | null
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
  /** Showing the in-page reader transform. In-memory only; reset on reload/launch. */
  reader: boolean
  /** Whether the page has a previous/next history entry. In-memory only;
   *  false on launch until the first load reports the real values. */
  can_go_back: boolean
  can_go_forward: boolean
  /** The page's load failure, when the last navigation failed. In-memory only;
   *  the chrome shows an error page in the tab's place, cleared on next load. */
  load_error: LoadError | null
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
  lockdown: boolean
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

/** Where a freshly opened tab lands. */
export type NewTabTarget = 'launchpad' | 'homepage'

/** An editable launchpad shortcut tile. */
export interface ShortcutTile {
  label: string
  url: string
}

/** Categories an export includes (mirror of the Rust `BackupOptions`). */
export interface BackupOptions {
  settings: boolean
  /** Workspaces + their tabs + bookmarks. */
  profile: boolean
  /** Browsing history (only meaningful alongside `profile`). */
  history: boolean
  /** Saved passwords (Touch ID required to decrypt them). */
  vault: boolean
  /** Canvases (only meaningful alongside `profile`). */
  canvases: boolean
}

/**
 * What an import applied (mirror of the Rust `ImportSummary`). The profile
 * graph is applied natively; `settings` is returned for the frontend to apply
 * through the settings commands.
 */
export interface ImportSummary {
  settings?: UserSettings | null
  workspaces_added: number
  bookmarks_added: number
  history_added: number
  canvases_added: number
  passwords_added: number
}

/** AI provider wire protocol (mirror of `wis_core::AiProvider`). */
export type AiProvider = 'anthropic' | 'open_ai_compatible'

/**
 * BYOK AI provider configuration (mirror of `wis_core::AiSettings`). The
 * feature is dormant until a key is set.
 */
export interface AiSettings {
  provider: AiProvider
  /** Model identifier sent to the provider. */
  model: string
  /** Base URL for `open_ai_compatible` providers; null uses the default. */
  base_url: string | null
  /** Ask Anthropic for summarized reasoning (billed as output tokens). */
  show_reasoning: boolean
  /** Offer web search/fetch tools to OpenAI-compatible models. */
  web_access: boolean
  /** Offer tools that read this workspace's tabs, history and bookmarks. */
  local_data: boolean
  /** Let the model open, switch, close and group tabs, each approved by the user. */
  tab_actions: boolean
  /** The model canvas agents use; null follows `model`. */
  canvas_model?: string | null
}

/** One tool call requested by the model (mirror of `wis_core::ai::ToolCall`). */
export interface AiToolCall {
  id: string
  name: string
  /** Raw JSON string, exactly as the wire carries it. */
  arguments: string
}

/** A model the provider lists (mirror of `wis_core::ai_models::ModelInfo`). */
export interface AiModel {
  id: string
  name: string
}

/** One web search hit for the assistant (mirror of `ai_fetch::AiSearchResult`). */
export interface AiSearchResult {
  title: string
  url: string
  snippet: string
}

/** An open tab for the assistant, under a short id (mirror of `ai_local::AiTab`). */
export interface AiTab {
  id: string
  title: string
  url: string
  active: boolean
  pinned: boolean
}

/** A history match for the assistant (mirror of `ai_local::AiHistoryHit`). */
export interface AiHistoryHit {
  title: string
  url: string
  /** RFC 3339. */
  last_visited: string
  visits: number
}

/** A bookmark match for the assistant (mirror of `ai_local::AiBookmarkHit`). */
export interface AiBookmarkHit {
  title: string
  url: string
  folder: string | null
}

/** A readable tab resolved from the model's id (mirror of `ai_local::AiTabRead`). */
export interface AiTabRead {
  id: TabId
  title: string
  url: string
  pinned: boolean
}

/** Tabs put in a folder by the assistant (mirror of `ai_actions::AiGrouped`). */
export interface AiGrouped {
  folder: string
  created: boolean
  tabs: AiTabRead[]
}

/** A fetched page for the assistant (mirror of `ai_fetch::AiFetchResponse`). */
export interface AiFetchResponse {
  url: string
  title: string
  text: string
}

/** A streaming event from `ai_chat` (mirror of `wis_core::StreamEvent`). */
export type StreamEvent =
  | { type: 'delta'; text: string }
  | { type: 'reasoning'; text: string }
  | { type: 'tool_calls'; calls: AiToolCall[]; blocks?: unknown[] }
  | { type: 'truncated' }
  | { type: 'done' }
  | { type: 'error'; message: string }

/** An installed browser extension (mirror of `wis_core::extensions::InstalledExtension`). */
export interface InstalledExtension {
  id: string
  name: string
  version: string
  disabled_in: WorkspaceId[]
  allow_private: boolean
  pinned: boolean
  granted: string[]
  site_access: SiteAccess
  allowed_sites: string[]
  store_id: string | null
  /** An update that requires more than this version. */
  pending_update: { version: string; abilities: string[] } | null
  /** A listing the daily search found for an extension from a folder or `.zip`. */
  store_suggestion: { store_id: string; same_files: boolean } | null
  declined_listings: string[]
}

/** What the extensions menu offers on a Chrome Web Store item page (mirror of `extensions::store::StoreOffer`). */
export type StoreOffer =
  | { kind: 'add'; store_id: string }
  | { kind: 'link'; store_id: string; id: string; name: string }

/** What linking an extension to its listing did (mirror of `extensions::linking::Linked`). */
export type Linked = 'current' | 'updated' | 'waiting'

/** What a check for extension updates did, by extension name (mirror of `extensions::updates::UpdateSummary`). */
export interface UpdateSummary {
  updated: string[]
  waiting: string[]
  failed: string[]
}

export type SiteAccess = 'all' | 'allowed'

/** What an extension asks for, shown before installing it (mirror of `ExtensionReview`). */
export interface ExtensionReview {
  name: string
  version: string
  description: string
  abilities: string[]
  /** Permissions it asks for that Wis cannot give. */
  unsupported: string[]
  /** Its blocking rules on by default, when there are enough to warn about. */
  large_rule_set: number | null
}

/** An extension's toolbar action for the active tab (mirror of `extensions::ExtensionAction`). */
export interface ExtensionAction {
  id: string
  name: string
  label: string
  /** Empty when there is no badge. */
  badge: string
  enabled: boolean
  has_options: boolean
  /** A PNG data URL. */
  icon: string | null
  /** The active tab's host, when the extension runs only on allowed sites and wants this one. */
  site: { host: string; allowed: boolean } | null
}

/** A running extension asking for more access (mirror of `extensions::prompts::PermissionRequest`). */
export interface ExtensionPermissionRequest {
  request: number
  workspace_id: WorkspaceId
  name: string
  abilities: string[]
}

/** An extension copied in and waiting on its review (mirror of `extensions::StagedExtension`). */
export interface StagedExtension {
  token: string
  review: ExtensionReview
}

export interface UserSettings {
  layout: LayoutMode
  top_tabs_rail_side: RailSide
  /**
   * Max folder islands expanded at once in the top-tabs strip (accordion:
   * expanding past the limit collapses the least-recently-expanded).
   * 0 means no limit. Top-tabs UI only; the sidebar tree is unrestricted.
   */
  top_tabs_max_expanded_folders: number
  theme: ThemeMode
  accent: AccentColor
  glass_enabled: boolean
  sidebar_collapsible: boolean
  compact: boolean
  default_privacy_mode: PrivacyMode
  doh_resolver: string | null
  homepage: string | null
  /** Where a freshly opened tab lands: launchpad or the homepage. */
  new_tab_target: NewTabTarget
  /** Editable launchpad shortcut tiles, in display order. */
  shortcuts: ShortcutTile[]
  search_engine: SearchEngine
  /** User-defined engines; at most one `is_default`, winning over `search_engine`. */
  custom_search_engines: CustomSearchEngine[]
  /** Optional address-bar keywords for built-in engines; custom keywords win on collision. */
  builtin_search_keywords: Partial<Record<SearchEngine, string>>
  filter_list_auto_update: boolean
  url_bar_focus_glow: boolean
  /** Launch directly into a private (ephemeral) workspace. */
  always_private: boolean
  /** Discard background tab webviews after they idle. */
  tab_sleep_enabled: boolean
  /** Idle minutes before a hidden tab's webview is discarded. */
  tab_sleep_minutes: number
  /** Count active time per site (never in private workspaces). */
  usage_tracking: boolean
  /** Seconds without input before usage stops counting. */
  usage_idle_timeout_secs: number
  /** First day of the week in usage summaries. */
  week_start: WeekStart
  canvas_theme: CanvasTheme
  /** Persist (encrypted) login sessions across restarts; auto-expire after a cap. */
  persist_sessions: boolean
  /** WebKit fraudulent-website warning (Apple-proxied Safe Browsing). */
  fraud_warning_enabled: boolean
  /** Global Privacy Control. */
  global_privacy_control: boolean
  /** What media may start without a gesture; mirrors `AutoplayPolicy`. */
  autoplay_policy: AutoplayPolicy
  /** Per-host autoplay overrides, keyed by normalised host. */
  autoplay_site_overrides: Record<string, AutoplayPolicy>
  site_protection_overrides: Record<string, SiteProtection>
  /** Page zoom for sites without one of their own, as a whole percentage. */
  default_zoom_percent: number
  /** Remembered per-host page zoom, keyed by normalised host. */
  zoom_site_overrides: Record<string, number>
  media_extensions_enabled: boolean
  extension_store_search: boolean
  /** Float a playing video as an always-on-top mini player on tab switch. */
  video_popout_enabled: boolean
  /** Default user agent presented to remote pages. */
  user_agent_profile: UaProfile
  password_offer: PasswordOffer
  /** Per-host user agent overrides, keyed by `www.`-stripped host. */
  ua_site_overrides: Record<string, UaProfile>
  /** Remembered camera/mic consent, keyed `"<kind>|<origin>"` (true = allow). */
  media_permissions: Record<string, boolean>
  /** Remembered open-in-app decisions, keyed `"<scheme>|<host>"` (true = launch). */
  app_launch_permissions: Record<string, boolean>
  /** Installed extensions; changed only through the extension commands. */
  extensions: InstalledExtension[]
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
  /** Evict hidden panels oldest-first past the combined footprint cap. */
  widget_memory_cap_enabled: boolean
  /** Combined hidden-panel footprint cap, in MB. */
  widget_memory_cap_mb: number
  /** User filter-list subscription URLs (https), engine-only. */
  filter_subscriptions: string[]
  block_cookie_notices: boolean
  /** Fanboy's Annoyance list, which contains the cookie list. */
  block_annoyances: boolean
  /** Element-picker hide rules, keyed by host. */
  user_cosmetic_rules: Record<string, string[]>
  /** Tampermonkey-style user scripts. */
  userscripts: Userscript[]
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
  /** Chrome geometry and material. */
  design: ChromeDesign
  /** BYOK AI provider configuration. */
  ai: AiSettings
}

/**
 * Partial update for `UserSettings`. Fields left undefined mean "no change".
 * Setting a field replaces the corresponding `UserSettings` field.
 */
export interface UserSettingsPatch {
  layout?: LayoutMode
  top_tabs_rail_side?: RailSide
  top_tabs_max_expanded_folders?: number
  theme?: ThemeMode
  accent?: AccentColor
  glass_enabled?: boolean
  sidebar_collapsible?: boolean
  compact?: boolean
  default_privacy_mode?: PrivacyMode
  doh_resolver?: string
  homepage?: string
  new_tab_target?: NewTabTarget
  shortcuts?: ShortcutTile[]
  search_engine?: SearchEngine
  /** Replaces the whole custom-engine list (the UI sends the full list). */
  custom_search_engines?: CustomSearchEngine[]
  /** Replaces the whole built-in keyword map. */
  builtin_search_keywords?: Partial<Record<SearchEngine, string>>
  filter_list_auto_update?: boolean
  url_bar_focus_glow?: boolean
  always_private?: boolean
  tab_sleep_enabled?: boolean
  tab_sleep_minutes?: number
  usage_tracking?: boolean
  usage_idle_timeout_secs?: number
  week_start?: WeekStart
  canvas_theme?: CanvasTheme
  persist_sessions?: boolean
  fraud_warning_enabled?: boolean
  global_privacy_control?: boolean
  autoplay_policy?: AutoplayPolicy
  autoplay_site_overrides?: Record<string, AutoplayPolicy>
  site_protection_overrides?: Record<string, SiteProtection>
  default_zoom_percent?: number
  zoom_site_overrides?: Record<string, number>
  media_extensions_enabled?: boolean
  extension_store_search?: boolean
  video_popout_enabled?: boolean
  user_agent_profile?: UaProfile
  password_offer?: PasswordOffer
  /** Replaces the whole override map when present. */
  ua_site_overrides?: Record<string, UaProfile>
  /** Replaces the whole remembered-permissions map when present. */
  media_permissions?: Record<string, boolean>
  app_launch_permissions?: Record<string, boolean>
  /** Replaces the whole widget list when present. */
  sidebar_widgets?: Widget[]
  widget_panel_limit_enabled?: boolean
  widget_panel_limit?: number
  widget_keep_loaded?: boolean
  widget_sleep_minutes?: number
  widget_memory_cap_enabled?: boolean
  widget_memory_cap_mb?: number
  filter_subscriptions?: string[]
  block_cookie_notices?: boolean
  block_annoyances?: boolean
  user_cosmetic_rules?: Record<string, string[]>
  userscripts?: Userscript[]
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
  /** Replaces the whole design when present (clamped boundary-side). */
  design?: ChromeDesign
  ai?: AiSettings
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
  /** Dock-row override for a custom widget; null renders under the kind's own row. */
  group: WidgetGroup | null
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

/** A captured document, held for the source tab displaying it. */
export interface PageSource {
  url: string
  /** Concatenated, these are the capture. */
  chunks: string[]
  byteLen: number
  truncated: boolean
}

/** A password field a tab's page reported focusing, and what it accepts. */
export interface SignupFieldInfo {
  rules: string | null
  maxLength: number | null
}

/** Whether a signup form is offered a generated password. */
export type PasswordOffer = 'ask' | 'always' | 'never'

/** A link with its tracking parameters removed; mirrors `CleanedLink`. */
export interface CleanedLink {
  url: string
  /** Names that were removed. Empty means the link came back untouched. */
  removed: string[]
}

/** One saved password found in a public breach data set. */
export interface BreachFinding {
  id: number
  /** Times the password appears across known breaches. */
  count: number
}

/** What a breach check found; mirrors `BreachReport`. */
export interface BreachReport {
  /** Passwords checked. */
  checked: number
  /** Only the breached ones. */
  breached: BreachFinding[]
}

/** Metadata of a closed tab; mirrors `ClosedTabRecord`. */
export interface ClosedTabRecord {
  id: number
  workspace_id: WorkspaceId
  url: string
  /** Title at close; empty when the page never reported one. */
  title: string
  folder_id: TabFolderId | null
  /** Slot in the workspace's tab order at close; -1 means append. */
  position: number
  pinned: boolean
  closed_at: string
}

/** What media may start without a gesture; mirrors `wis_core::AutoplayPolicy`. */
export type AutoplayPolicy = 'allow' | 'block_audible' | 'block'

export interface SiteProtection {
  blocking: boolean
  javascript: boolean
  fingerprinting: boolean
}

export interface PageBlocks {
  total: number
  /** Most blocked first. */
  hosts: [string, number][]
}

/** One separable slice of a workspace's site data; mirrors `SiteDataKind`. */
export type SiteDataKind = 'cookies' | 'cache' | 'storage'

/** A domain with stored site data (cookies/cache/storage) in a workspace. */
export interface SiteDataEntry {
  domain: string
}

// ---------------------------------------------------------------------------
// Downloads (mirror of `downloads.rs` `DownloadItem`, camelCase serde).
// ---------------------------------------------------------------------------

export type DownloadStatus = 'in_progress' | 'completed' | 'failed' | 'cancelled'

export type RunAt = 'document-start' | 'document-end' | 'document-idle'

/** A Tampermonkey-style user script. */
export interface Userscript {
  id: string
  name: string
  enabled: boolean
  matches: string[]
  excludes: string[]
  run_at: RunAt
  source: string
}

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
  /** Content-Length total, or null when unknown (chunked, or non-macOS). */
  totalBytes: number | null
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

/** Outcome of a bookmarks HTML import (mirror of `BookmarkImportStats`). */
export interface BookmarkImportStats {
  folders_added: number
  bookmarks_added: number
  bookmarks_skipped: number
}

/** A browser profile found on this machine (mirror of `browser_import::ImportSource`). */
export interface ImportSource {
  id: string
  browser: string
  profile: string
  bookmarks: boolean
  history: boolean
}

/** Outcome of a history import (mirror of `HistoryImportStats`). */
export interface HistoryImportStats {
  added: number
  merged: number
}

/** Mirror of `wis_core::usage::WeekStart`; `auto` follows the region. */
export type WeekStart = 'auto' | 'monday' | 'sunday'

/** How canvases look, independent of the browser's theme. */
export type CanvasTheme = 'dark' | 'light'

/** Active time over a range of local days (mirror of `wis_core::usage::UsageSummary`). */
export interface UsageSummary {
  total_seconds: number
  /** Busiest first. */
  sites: { site: string; seconds: number }[]
  /** Seconds per local hour, 24 entries. */
  hours: number[]
  /** Every day of the range as `YYYY-MM-DD`, quiet days included. */
  days: { day: string; seconds: number }[]
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
/** Outcome of a password CSV import (mirror of `CsvImportStats`). */
export interface CsvImportStats {
  imported: number
  skipped: number
}

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
