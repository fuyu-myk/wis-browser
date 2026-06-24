// Typed wrappers around `@tauri-apps/api/core` invoke() for every command
// exposed by `crates/shell/src-tauri/src/commands.rs`.
//
// Tauri 2 convention: JS-side arg keys are camelCase. The Rust-side function
// param `workspace_id: WorkspaceId` corresponds to `workspaceId` on the JS
// side. Passing snake_case from JS silently fails to deserialise.

import { invoke } from '@tauri-apps/api/core'
import type {
  AccentColor,
  Background,
  BackupOptions,
  ImportSummary,
  Bookmark,
  BookmarkFolder,
  BrowserState,
  CredentialSummary,
  CustomTheme,
  Decision,
  DownloadId,
  DownloadItem,
  HistoryEntry,
  HistoryQuery,
  LinkAssessment,
  PaneBranch,
  SiteDataEntry,
  PrivacyMode,
  RequestType,
  SplitDir,
  Tab,
  TabFolder,
  TabFolderId,
  TabId,
  ThemeId,
  UpstreamProxy,
  UrlSuggestions,
  UserSettings,
  UserSettingsPatch,
  VaultStatus,
  WidgetId,
  Workspace,
  WorkspaceId,
  WebviewMemory,
  Userscript,
} from '@/bindings/types'

export const commands = {
  // -- queries --------------------------------------------------------------

  async getBrowserState(): Promise<BrowserState> {
    return invoke('get_browser_state')
  },

  async getWorkspace(id: WorkspaceId): Promise<Workspace> {
    return invoke('get_workspace', { id })
  },

  async getTab(id: TabId): Promise<Tab> {
    return invoke('get_tab', { id })
  },

  /** Query browsing history, newest-first. `query` filters and paginates;
   *  `workspace_id` scopes to one workspace (null = all). */
  async getHistory(query: Partial<HistoryQuery> = {}): Promise<HistoryEntry[]> {
    const full: HistoryQuery = {
      search: query.search ?? '',
      workspace_id: query.workspace_id ?? null,
      limit: query.limit ?? 0,
      offset: query.offset ?? 0,
    }
    return invoke('get_history', { query: full })
  },

  /** Delete specific history rows by id. */
  async deleteHistory(ids: number[]): Promise<void> {
    return invoke('delete_history', { ids })
  },

  /** Remove browsing history; `workspaceId` scopes the wipe (omit = all). */
  async clearHistory(workspaceId?: WorkspaceId): Promise<void> {
    return invoke('clear_history', { workspaceId: workspaceId ?? null })
  },

  /**
   * Wipe a workspace's cookies, cache and local storage (all site data) from
   * its persistent WebKit data store.
   */
  async clearSiteData(workspaceId: WorkspaceId): Promise<void> {
    return invoke('clear_site_data', { workspaceId })
  },

  /** List the domains with stored site data in a workspace, sorted. */
  async listSiteData(workspaceId: WorkspaceId): Promise<SiteDataEntry[]> {
    return invoke('list_site_data', { workspaceId })
  },

  /** Remove all stored site data for one `domain` in a workspace. */
  async deleteSiteData(workspaceId: WorkspaceId, domain: string): Promise<void> {
    return invoke('delete_site_data', { workspaceId, domain })
  },

  // -- password vault -------------------------------------------------------

  /** Vault lock/setup/support status. */
  async vaultStatus(): Promise<VaultStatus> {
    return invoke('vault_status')
  },

  /** Touch ID unlock (creates the vault on first use). Returns unlocked state. */
  async vaultUnlock(): Promise<boolean> {
    return invoke('vault_unlock')
  },

  /** Drop the in-memory master key (re-lock). */
  async vaultLock(): Promise<void> {
    return invoke('vault_lock')
  },

  /** List saved credentials (no passwords), optionally filtered to one host. */
  async vaultList(host?: string): Promise<CredentialSummary[]> {
    return invoke('vault_list', { host: host ?? null })
  },

  /** Save or update a credential (encrypts the password). */
  async vaultSave(host: string, username: string, password: string): Promise<void> {
    return invoke('vault_save', { host, username, password })
  },

  /** Decrypt and return one saved password (vault must be unlocked). */
  async vaultReveal(id: number): Promise<string> {
    return invoke('vault_reveal', { id })
  },

  /** Delete a saved credential. */
  async vaultDelete(id: number): Promise<void> {
    return invoke('vault_delete', { id })
  },

  /** Whether a login is saved for `host`. */
  async vaultHasCredentials(host: string): Promise<boolean> {
    return invoke('vault_has_credentials', { host })
  },

  /** Saved logins for `host` (id + username only, no unlock) for the picker. */
  async autofillCandidates(host: string): Promise<CredentialSummary[]> {
    return invoke('autofill_candidates', { host })
  },

  /** Fill a tab's login form with a saved credential (password injected
   *  backend-side; vault must be unlocked and the host must match). */
  async autofillFill(tabId: TabId, credentialId: number): Promise<void> {
    return invoke('autofill_fill', { tabId, credentialId })
  },

  // -- URL-bar suggestions --------------------------------------------------

  /** Suggestions for the address bar: past searches + ranked sites for the
   *  workspace, both filtered by `query`. */
  async getUrlSuggestions(query: string, workspaceId: WorkspaceId): Promise<UrlSuggestions> {
    return invoke('get_url_suggestions', { query, workspaceId })
  },

  /** Remember a search (no-op in private workspaces). */
  async recordSearch(query: string): Promise<void> {
    return invoke('record_search', { query })
  },

  /** Forget a remembered search. */
  async deleteSearch(query: string): Promise<void> {
    return invoke('delete_search', { query })
  },

  // -- "Save password?" capture ----------------------------------

  /** Encrypt + save the pending captured login (vault must be unlocked). */
  async captureSave(): Promise<void> {
    return invoke('capture_save')
  },

  /** Discard the pending captured login. */
  async captureDismiss(): Promise<void> {
    return invoke('capture_dismiss')
  },

  /** The pending captured login (host + username, no password), or null. */
  async capturePending(): Promise<{ host: string; username: string } | null> {
    return invoke('capture_pending')
  },

  async getSettings(): Promise<UserSettings> {
    return invoke('get_settings')
  },

  // -- workspace mutations --------------------------------------------------

  async createWorkspace(name: string, accent: AccentColor): Promise<Workspace> {
    return invoke('create_workspace', { name, accent })
  },

  /**
   * Create a private (ephemeral) workspace. Its tabs use a non-persistent data
   * store and the workspace is never written to disk.
   */
  async createPrivateWorkspace(name: string, accent: AccentColor): Promise<Workspace> {
    return invoke('create_private_workspace', { name, accent })
  },

  async renameWorkspace(id: WorkspaceId, name: string): Promise<Workspace> {
    return invoke('rename_workspace', { id, name })
  },

  async deleteWorkspace(id: WorkspaceId): Promise<void> {
    return invoke('delete_workspace', { id })
  },

  async recolorWorkspace(id: WorkspaceId, accent: AccentColor): Promise<Workspace> {
    return invoke('recolor_workspace', { id, accent })
  },

  /** Set or clear a folder's icon (encoded `glyph:`/`glyph-fill:`/`emoji:`). */
  async setTabFolderIcon(
    workspaceId: WorkspaceId,
    folderId: TabFolderId,
    icon: string | null,
  ): Promise<void> {
    return invoke('set_tab_folder_icon', { workspaceId, folderId, icon })
  },

  /** Set or clear a workspace's icon (encoded `glyph:`/`glyph-fill:`/`emoji:`). */
  async setWorkspaceIcon(id: WorkspaceId, icon: string | null): Promise<void> {
    return invoke('set_workspace_icon', { id, icon })
  },

  /** Set or clear a workspace's custom flat dot colour (null = preset). */
  async setWorkspaceCustomAccent(
    id: WorkspaceId,
    color: string | null,
  ): Promise<Workspace> {
    return invoke('set_workspace_custom_accent', { id, color })
  },

  /** Native image picker -> wallpaper store; resolves the asset hash, or
   *  null when the user cancels. */
  async importWallpaper(): Promise<string | null> {
    return invoke('import_wallpaper')
  },

  /** Set or clear a workspace's custom background (null = theme default). */
  async setWorkspaceBackground(
    id: WorkspaceId,
    background: Background | null,
  ): Promise<Workspace> {
    return invoke('set_workspace_background', { id, background })
  },

  async setActiveWorkspace(id: WorkspaceId): Promise<void> {
    return invoke('set_active_workspace', { id })
  },

  /**
   * Change a specific workspace's privacy mode. The server rebuilds the proxy
   * and re-applies the engine content filter to open tabs in the workspace.
   */
  async setWorkspacePrivacyMode(id: WorkspaceId, mode: PrivacyMode): Promise<Workspace> {
    return invoke('set_workspace_privacy_mode', { id, mode })
  },

  /**
   * Set (or clear, with `null`) a workspace's upstream proxy.
   */
  async setWorkspaceUpstream(
    id: WorkspaceId,
    upstream: UpstreamProxy | null,
  ): Promise<Workspace> {
    return invoke('set_workspace_upstream', { id, upstream })
  },

  /** Probe an upstream proxy for reachability. Rejects with the failure reason. */
  async testUpstream(upstream: UpstreamProxy): Promise<void> {
    return invoke('test_upstream', { upstream })
  },

  /** Whether the built-in Tor client is compiled into this build. */
  async torAvailable(): Promise<boolean> {
    return invoke('tor_available')
  },

  // -- tab mutations --------------------------------------------------------

  async createTab(workspaceId: WorkspaceId, url: string): Promise<Tab> {
    return invoke('create_tab', { workspaceId, url })
  },

  /**
   * Like {@link createTab} but the new tab is always loose (outside any folder),
   * ignoring the workspace's selected folder. Backs the explicit "+ New tab"
   * button.
   */
  async createLooseTab(workspaceId: WorkspaceId, url: string): Promise<Tab> {
    return invoke('create_loose_tab', { workspaceId, url })
  },

  /**
   * Split the workspace's focused pane along `dir`, opening `url` in the new
   * pane and focusing it. `placeNewFirst` puts the new pane in the left/top
   * slot. Returns the created tab.
   */
  async splitPane(
    workspaceId: WorkspaceId,
    dir: SplitDir,
    placeNewFirst: boolean,
    url: string,
  ): Promise<Tab> {
    return invoke('split_pane', { workspaceId, dir, placeNewFirst, url })
  },

  /**
   * Set the divider ratio (permille, 0..1000) of the split reached by `path`
   * from the workspace's pane-tree root.
   */
  async setPaneRatio(workspaceId: WorkspaceId, path: PaneBranch[], ratio: number): Promise<void> {
    return invoke('set_pane_ratio', { workspaceId, path, ratio })
  },

  /** Collapse all splits in a workspace back to a single pane (focused tab). */
  async unsplitPanes(workspaceId: WorkspaceId): Promise<void> {
    return invoke('unsplit_panes', { workspaceId })
  },

  /**
   * Split the `targetTab` pane along `dir` with the already-open `movedTab`
   * (dragged from the tab list onto a pane). `placeNewFirst` puts the moved tab
   * in the left/top slot.
   */
  async splitPaneWith(
    workspaceId: WorkspaceId,
    targetTab: TabId,
    dir: SplitDir,
    placeNewFirst: boolean,
    movedTab: TabId,
  ): Promise<void> {
    return invoke('split_pane_with', { workspaceId, targetTab, dir, placeNewFirst, movedTab })
  },

  /**
   * Replace the `targetTab` pane's tab with the already-open `movedTab`
   * (dragged onto a pane's centre). A throwaway placeholder in that pane is
   * closed; a real page is kept as a background tab.
   */
  async replacePaneLeaf(
    workspaceId: WorkspaceId,
    targetTab: TabId,
    movedTab: TabId,
  ): Promise<void> {
    return invoke('replace_pane_leaf', { workspaceId, targetTab, movedTab })
  },

  async closeTab(id: TabId): Promise<void> {
    return invoke('close_tab', { id })
  },

  /** Reopen the most-recently-closed tab (in its original workspace). */
  async restoreClosedTab(): Promise<void> {
    return invoke('restore_closed_tab')
  },

  /** Open `url` in a new tab without switching to it. */
  async createBackgroundTab(workspaceId: WorkspaceId, url: string): Promise<Tab> {
    return invoke('create_background_tab', { workspaceId, url })
  },

  /** Set the swipe-nav indicator colour; stored for new tabs and
   *  pushed into every live tab webview. */
  async setSwipeAccent(color: string): Promise<void> {
    return invoke('set_swipe_accent', { color })
  },

  /** Detach a tab into a new workspace shown in its own window. `name`/`accent`
   *  seed the new workspace; a private tab detaches into a new private window. */
  async moveTabToNewWindow(tabId: TabId, name: string, accent: AccentColor): Promise<void> {
    return invoke('move_tab_to_new_window', { tabId, name, accent })
  },

  /** Move a tab into an existing workspace. Refuses to move a private tab into a
   *  persistent workspace; the session (history/scroll) is carried across. */
  async moveTabToWorkspace(tabId: TabId, workspaceId: WorkspaceId): Promise<void> {
    return invoke('move_tab_to_workspace', { tabId, workspaceId })
  },

  /** Reload a tab bypassing the HTTP cache. */
  async tabHardReload(id: TabId): Promise<void> {
    return invoke('tab_hard_reload', { id })
  },

  /** Adjust a tab's page zoom: `'in' | 'out' | 'reset'`. */
  async tabZoom(id: TabId, kind: 'in' | 'out' | 'reset'): Promise<void> {
    return invoke('tab_zoom', { id, kind })
  },

  async reorderTabs(workspaceId: WorkspaceId, order: TabId[]): Promise<void> {
    return invoke('reorder_tabs', { workspaceId, order })
  },

  async pinTab(id: TabId, pinned: boolean): Promise<Tab> {
    return invoke('pin_tab', { id, pinned })
  },

  /** Copy plain text to the system clipboard (native pasteboard). */
  async copyText(text: string): Promise<void> {
    return invoke('copy_text', { text })
  },

  /** Per-webview WebContent process memory footprints (task manager). */
  async listWebviewMemory(): Promise<WebviewMemory[]> {
    return invoke('list_webview_memory')
  },

  // -- tab folders ----------------------------------------------------------

  async createTabFolder(
    workspaceId: WorkspaceId,
    name: string,
    accent: AccentColor,
  ): Promise<TabFolder> {
    return invoke('create_tab_folder', { workspaceId, name, accent })
  },
  async renameTabFolder(
    workspaceId: WorkspaceId,
    folderId: TabFolderId,
    name: string,
  ): Promise<void> {
    return invoke('rename_tab_folder', { workspaceId, folderId, name })
  },
  async recolorTabFolder(
    workspaceId: WorkspaceId,
    folderId: TabFolderId,
    accent: AccentColor,
  ): Promise<void> {
    return invoke('recolor_tab_folder', { workspaceId, folderId, accent })
  },
  async setTabFolderCollapsed(
    workspaceId: WorkspaceId,
    folderId: TabFolderId,
    collapsed: boolean,
  ): Promise<void> {
    return invoke('set_tab_folder_collapsed', { workspaceId, folderId, collapsed })
  },
  async deleteTabFolder(workspaceId: WorkspaceId, folderId: TabFolderId): Promise<void> {
    return invoke('delete_tab_folder', { workspaceId, folderId })
  },
  async reorderTabFolders(workspaceId: WorkspaceId, order: TabFolderId[]): Promise<void> {
    return invoke('reorder_tab_folders', { workspaceId, order })
  },
  async assignTabToFolder(tabId: TabId, folderId: TabFolderId | null): Promise<void> {
    return invoke('assign_tab_to_folder', { tabId, folderId })
  },
  async selectTabFolder(workspaceId: WorkspaceId, folderId: TabFolderId | null): Promise<void> {
    return invoke('select_tab_folder', { workspaceId, folderId })
  },

  async setActiveTab(id: TabId): Promise<void> {
    return invoke('set_active_tab', { id })
  },

  /**
   * Navigate an existing tab's child webview to `url`. State is updated
   * even if the webview hasn't been spun up yet: the next reconcile
   * pass picks up the new URL.
   */
  async navigateTab(id: TabId, url: string): Promise<void> {
    return invoke('navigate_tab', { id, url })
  },

  /**
   * Apply a CSS-style corner radius to a tab's native webview surface so its
   * edges match the rounded chrome. macOS only; other platforms no-op.
   */
  async setWebviewCornerRadius(label: string, radius: number): Promise<void> {
    return invoke('set_webview_corner_radius', { label, radius })
  },

  /**
   * Create a tab's child webview server-side so the per-workspace
   * `WKWebsiteDataStore` identifier is actually applied.
   */
  async createTabWebview(args: {
    windowLabel: string
    label: string
    url: string
    workspaceId: WorkspaceId
    tabId: TabId
    x: number
    y: number
    width: number
    height: number
    /**
     * Serialized WKWebView `interactionState` from a slept webview (read via
     * `tabInteractionState`); when set, the webview starts on about:blank
     * and the restore performs the navigation with history, scroll, and
     * form state intact.
     */
    restoreState: string | null
  }): Promise<void> {
    return invoke('create_tab_webview', args)
  },

  /**
   * Read a tab webview's serialized session (WKWebView `interactionState`,
   * base64) for restoring after sleep. Null when unavailable.
   */
  async tabInteractionState(id: TabId): Promise<string | null> {
    return invoke('tab_interaction_state', { id })
  },

  /**
   * Persist a tab's serialized session (WKWebView `interactionState`, base64)
   * to disk, so its back/forward history + scroll survive a full restart.
   * Called just before a background webview is discarded (sleep). Private tabs
   * are skipped backend-side.
   */
  async persistTabSession(tabId: TabId, stateB64: string): Promise<void> {
    return invoke('persist_tab_session', { tabId, stateB64 })
  },

  // -- downloads --------------------------------------------------------------

  /** Downloads newest first: this session's items, then persisted history. */
  async listDownloads(): Promise<DownloadItem[]> {
    return invoke('list_downloads')
  },

  /** Remove one download from the list and history (the file is untouched). */
  async removeDownload(id: DownloadId): Promise<void> {
    return invoke('remove_download', { id })
  },

  /** Remove several downloads from the list and history (files untouched). */
  async removeDownloads(ids: DownloadId[]): Promise<void> {
    return invoke('remove_downloads', { ids })
  },

  /**
   * Open a completed download with its default app. Resolves `false` when
   * the file is no longer at its recorded path (moved or deleted).
   */
  async openDownload(id: DownloadId): Promise<boolean> {
    return invoke('open_download', { id })
  },

  /** Reveal a completed download in Finder. Same semantics as `openDownload`. */
  async revealDownload(id: DownloadId): Promise<boolean> {
    return invoke('reveal_download', { id })
  },

  /**
   * Set a tab webview's position and size atomically in one IPC round-trip.
   */
  async setTabWebviewBounds(
    label: string,
    x: number,
    y: number,
    width: number,
    height: number,
  ): Promise<void> {
    return invoke('set_tab_webview_bounds', { label, x, y, width, height })
  },

  /** Read the current URL of a tab's native webview by label. */
  async webviewUrl(label: string): Promise<string | null> {
    return invoke('webview_url', { label })
  },

  /**
   * Sync a tab's stored URL to where the webview actually navigated
   * (in-page link clicks, history hops). State-only; does not call
   * `webview.navigate` again: see `navigateTab` for more information.
   */
  async syncTabUrl(id: TabId, url: string): Promise<void> {
    return invoke('sync_tab_url', { id, url })
  },

  // -- nav controls + state pokes ---------------------------------

  /** Update a tab's title from the chrome side. */
  async updateTabTitle(id: TabId, title: string): Promise<void> {
    return invoke('update_tab_title', { id, title })
  },

  /** Explicitly set a tab's loading flag. */
  async updateTabLoading(id: TabId, loading: boolean): Promise<void> {
    return invoke('update_tab_loading', { id, loading })
  },

  /** Reload the tab's native webview. */
  async tabReload(id: TabId): Promise<void> {
    return invoke('tab_reload', { id })
  },

  /** Toggle reader mode on a tab. Enter transforms the page; exit reloads it. */
  async tabSetReader(id: TabId, enabled: boolean): Promise<void> {
    return invoke('tab_set_reader', { id, enabled })
  },

  /** History back in the tab. No-ops at start of history. */
  async tabGoBack(id: TabId): Promise<void> {
    return invoke('tab_go_back', { id })
  },

  /** History forward in the tab. No-ops at end of history. */
  async tabGoForward(id: TabId): Promise<void> {
    return invoke('tab_go_forward', { id })
  },

  /**
   * Open the macOS print panel for the tab (Cmd+P).
   */
  async printActiveTab(id: TabId): Promise<void> {
    return invoke('print_active_tab', { id })
  },

  /**
   * Capture and copy to the clipboard, holding it for a later Save. `mode`:
   * 'visible' (tab content), 'window' (whole browser frame), or 'region' (a
   * `rect` crop of the window, in content-area CSS px from the selection
   * overlay). Resolves true when a capture was made.
   */
  async captureToClipboard(
    id: TabId,
    mode: 'visible' | 'region' | 'window',
    rect?: { x: number; y: number; width: number; height: number },
  ): Promise<boolean> {
    return invoke('capture_to_clipboard', { id, mode, rect: rect ?? null })
  },

  /** Save the most recent screenshot to a chosen file. Null if none / cancelled. */
  async saveLastScreenshot(): Promise<string | null> {
    return invoke('save_last_screenshot')
  },

  /**
   * Open an http(s) URL in the OS default browser (a full, separate browser).
   * An exit hatch for pages the embedded engine can't fully handle, e.g.
   * Widevine-protected video.
   */
  async openInExternalBrowser(url: string): Promise<void> {
    return invoke('open_in_external_browser', { url })
  },

  /**
   * Open widget `id`'s quick-launch panel: a floating webview above the tab
   * webviews, sharing the active workspace's data store and proxy. Reveals
   * the existing panel when it is already live.
   */
  async openWidgetPanel(
    id: WidgetId,
    url: string,
    x: number,
    y: number,
    width: number,
    height: number,
    dark: boolean,
  ): Promise<void> {
    return invoke('open_widget_panel', { id, url, x, y, width, height, dark })
  },

  /**
   * Re-show widget `id`'s hidden panel without navigating (in-page state
   * kept). Returns false when it isn't live; the caller should open it.
   */
  async showWidgetPanel(
    id: WidgetId,
    x: number,
    y: number,
    width: number,
    height: number,
  ): Promise<boolean> {
    return invoke('show_widget_panel', { id, x, y, width, height })
  },

  /** Hide widget `id`'s panel, keeping the page alive for instant re-show. */
  async hideWidgetPanel(id: WidgetId): Promise<void> {
    return invoke('hide_widget_panel', { id })
  },

  /** Reposition/resize widget `id`'s panel. No-op when it isn't live. */
  async setWidgetPanelBounds(
    id: WidgetId,
    x: number,
    y: number,
    width: number,
    height: number,
  ): Promise<void> {
    return invoke('set_widget_panel_bounds', { id, x, y, width, height })
  },

  /** Close widget `id`'s panel. No-op when it isn't live. */
  async closeWidgetPanel(id: WidgetId): Promise<void> {
    return invoke('close_widget_panel', { id })
  },

  /** Close every live widget panel (workspace switch, dock unmount). */
  async closeAllWidgetPanels(): Promise<void> {
    return invoke('close_all_widget_panels')
  },

  /** Widget ids of the live panel webviews (visible or hidden). */
  async listWidgetPanels(): Promise<WidgetId[]> {
    return invoke('list_widget_panels')
  },

  /**
   * Hard-reload widget `id`'s panel (native, cache-bypassing). Works even
   * when the page is hung and injected JS would never run.
   */
  async reloadWidgetPanel(id: WidgetId): Promise<void> {
    return invoke('reload_widget_panel', { id })
  },

  /** Navigate widget `id`'s live panel (e.g. re-applying a theme-dependent
   *  query parameter). No-op when the panel isn't live. */
  async navigateWidgetPanel(id: WidgetId, url: string): Promise<void> {
    return invoke('navigate_widget_panel', { id, url })
  },

  // -- video popout -----------------------------------------------------------

  /**
   * Pop tab webview `label`'s playing video out into the floating
   * always-on-top mini player. The page confirms via the native enter
   * handshake; without a suitable video the webview just stays hidden.
   * `geometry` is the remembered position of the previous popout, if any;
   * the size always starts from the predefined default.
   */
  async openVideoPopout(
    label: string,
    geometry: { x: number; y: number } | null,
  ): Promise<void> {
    return invoke('open_video_popout', {
      label,
      x: geometry?.x ?? null,
      y: geometry?.y ?? null,
    })
  },

  /**
   * Restore tab webview `label` out of the popout (styling unwound, webview
   * back in the main window, hidden). Safe to call in any popout phase.
   */
  async closeVideoPopout(label: string): Promise<void> {
    return invoke('close_video_popout', { label })
  },

  // -- tab audio --------------------------------------------------------------

  /** Mute or unmute a tab's page (engine-level; survives sleep/wake). */
  async setTabMuted(id: TabId, muted: boolean): Promise<void> {
    return invoke('set_tab_muted', { id, muted })
  },

  /** Current audible + muted tab uuids, for the chrome's initial render. */
  async tabAudioState(): Promise<{ audible: string[]; muted: string[] }> {
    return invoke('tab_audio_state')
  },

  /** Mute or unmute widget `id`'s panel page (pings stay silent too). */
  async setWidgetPanelMuted(id: WidgetId, muted: boolean): Promise<void> {
    return invoke('set_widget_panel_muted', { id, muted })
  },

  /** Whether widget `id` is currently muted. */
  async widgetPanelMuted(id: WidgetId): Promise<boolean> {
    return invoke('widget_panel_muted', { id })
  },

  // -- custom themes ----------------------------------------------------------

  /** Activate a custom theme (null = built-in presets). */
  async setActiveTheme(id: ThemeId | null): Promise<void> {
    return invoke('set_active_theme', { id })
  },

  /** Save a theme as a .wistheme file. Resolves false on cancel. */
  async exportTheme(theme: CustomTheme): Promise<boolean> {
    return invoke('export_theme', { theme })
  },

  /** Open and validate a .wistheme file; null on cancel. The returned theme
   *  carries a fresh id and is not yet in settings. */
  async importTheme(): Promise<CustomTheme | null> {
    return invoke('import_theme')
  },

  /** Encode a theme as a portable share code (`wis-theme:<base64>`). */
  async encodeThemeCode(theme: CustomTheme): Promise<string> {
    return invoke('encode_theme_code', { theme })
  },

  /** Decode a share code into a theme with a fresh id; rejects malformed input. */
  async decodeThemeCode(code: string): Promise<CustomTheme> {
    return invoke('decode_theme_code', { code })
  },

  // -- encrypted backup -----------------------------------------------------

  /** Export the selected categories to a passphrase-encrypted `.wisbackup`
   *  file via a save panel. Resolves false on cancel. */
  async exportBackup(passphrase: string, options: BackupOptions): Promise<boolean> {
    return invoke('export_backup', { passphrase, options })
  },

  /** Open a `.wisbackup` file, decrypt it with `passphrase`, apply the profile
   *  graph natively, and return a summary (incl. the settings to apply). Null
   *  on cancel; throws on a wrong passphrase or a corrupted file. */
  async importBackup(passphrase: string): Promise<ImportSummary | null> {
    return invoke('import_backup', { passphrase })
  },

  // -- settings -------------------------------------------------------------

  async updateSettings(patch: UserSettingsPatch): Promise<UserSettings> {
    return invoke('update_settings', { patch })
  },

  /** Re-apply the saved keyboard shortcuts to the native menu on demand. */
  async applyShortcuts(): Promise<void> {
    return invoke('apply_shortcuts')
  },

  // -- privacy engine -------------------------------------------------------

  /**
   * Ask the privacy engine what it would do with `url` requested from
   * `sourceUrl`. This inspector is read-only.
   */
  async checkUrlPolicy(
    url: string,
    sourceUrl: string,
    requestType: RequestType,
  ): Promise<Decision> {
    return invoke('check_url_policy', {
      url,
      sourceUrl,
      requestType,
    })
  },

  /**
   * Resolve `hostname` via the configured DoH resolver. Returns IPv4 / IPv6
   * addresses as strings. Useful for verifying DoH connectivity from
   * devtools.
   */
  async resolveHostname(hostname: string): Promise<string[]> {
    return invoke('resolve_hostname', { hostname })
  },

  /**
   * Swap the OS-level window / dock icon for the given window. `"open"` is
   * the at-rest owl; `"closed"` is the eyes-closed variant for private or
   * reader contexts. Returns when the platform has accepted the new icon.
   */
  async setWindowIcon(windowLabel: string, variant: 'open' | 'closed'): Promise<void> {
    return invoke('set_window_icon', { windowLabel, variant })
  },

  /**
   * Open a new OS window showing an existing (free) workspace. One workspace
   * per window; the chrome bundle resolves its own workspace from the window
   * label. Returns the new window's label.
   */
  async newWindow(workspaceId: WorkspaceId): Promise<string> {
    return invoke('new_window', { workspaceId })
  },

  /** Persistent workspaces not currently shown by any window, for the New
   *  Window picker (one workspace per window). */
  async freeWorkspaces(): Promise<Workspace[]> {
    return invoke('free_workspaces')
  },

  /** Reopen the multi-window layout saved at the previous exit. Runs once per
   *  process; the main window calls it on boot. */
  async restoreSessionWindows(): Promise<void> {
    return invoke('restore_session_windows')
  },

  /**
   * Set the macOS Dock (application) icon. `'closed'` shows the private-mode
   * owl; `'open'` clears the override so the system restores the bundled icon.
   */
  async setDockIcon(variant: 'open' | 'closed'): Promise<void> {
    return invoke('set_dock_icon', { variant })
  },

  /**
   * Refresh the EasyPrivacy filter list from upstream and hot-swap it into
   * the running tracker engine. Returns the size of the new list in bytes.
   * Live tabs are re-attached with the new rules in the same call.
   */
  async refreshFilterLists(): Promise<string> {
    return invoke('refresh_filter_lists')
  },

  /** Start the element picker ("zapper") on a tab's webview. */
  async startElementPicker(id: TabId): Promise<void> {
    return invoke('start_element_picker', { id })
  },

  /** Import a `.user.js` file via the native open panel (null = cancelled). */
  async importUserscript(): Promise<Userscript | null> {
    return invoke('import_userscript')
  },

  /**
   * Find a string in the tab's webview. `backwards = false` searches
   * forward (Cmd+G); `backwards = true` searches backward (Cmd+Shift+G).
   * Match status is published asynchronously via the `wis-find-result`
   * Tauri event.
   */
  async tabFindString(
    id: TabId,
    query: string,
    backwards: boolean,
    caseSensitive: boolean,
  ): Promise<void> {
    return invoke('tab_find_string', { id, query, backwards, caseSensitive })
  },

  /** Clear the tab's find highlights. */
  async tabFindClear(id: TabId): Promise<void> {
    return invoke('tab_find_clear', { id })
  },

  /**
   * Promote a named webview to its window's first responder so a subsequent
   * DOM `.focus()` on its UI receives keystrokes even while a page webview
   * currently has focus. Pass `"main"` for the chrome, `"overlay-<name>"`
   * for an overlay, or a tab label.
   */
  async focusWebview(label: string): Promise<void> {
    return invoke('focus_webview', { label })
  },

  /**
   * Make the calling webview its window's first responder. Window-agnostic
   * (no label), so a chrome or overlay surface focuses itself on whichever OS
   * window it lives on.
   */
  async focusSelf(): Promise<void> {
    return invoke('focus_self')
  },

  /**
   * Lazily create the named overlay webview (loads
   * `index.html?surface=<name>`). Idempotent. The overlay starts hidden
   * and must be revealed by `setOverlayBounds`.
   */
  async ensureOverlay(name: string): Promise<void> {
    return invoke('ensure_overlay', { name })
  },

  /** Position/resize the overlay and make it visible. */
  async setOverlayBounds(
    name: string,
    x: number,
    y: number,
    width: number,
    height: number,
  ): Promise<void> {
    return invoke('set_overlay_bounds', { name, x, y, width, height })
  },

  /** Move an already-shown overlay without resizing/re-showing it, for smooth
   *  per-frame entrance animation. */
  async setOverlayPosition(name: string, x: number, y: number): Promise<void> {
    return invoke('set_overlay_position', { name, x, y })
  },

  /** Position/resize the overlay while keeping it hidden, so a later show
   *  does not resize the webview mid-animation. */
  async sizeOverlay(
    name: string,
    x: number,
    y: number,
    width: number,
    height: number,
  ): Promise<void> {
    return invoke('size_overlay', { name, x, y, width, height })
  },

  /** Hide the overlay (kept around for the next show). */
  async hideOverlay(name: string): Promise<void> {
    return invoke('hide_overlay', { name })
  },

  // -- peek (Opt+click link preview) ----------------------------------------

  /**
   * Open (or replace) the isolated link-preview webview at the given rect and
   * return the link's safety assessment. The preview is incognito and routed
   * through the active workspace proxy.
   */
  async openPeek(args: {
    url: string
    anchorText: string | null
    x: number
    y: number
    width: number
    height: number
  }): Promise<LinkAssessment> {
    return invoke('open_peek', {
      url: args.url,
      anchorText: args.anchorText,
      x: args.x,
      y: args.y,
      width: args.width,
      height: args.height,
    })
  },

  /** Reposition/resize the preview webview. No-op if none is open. */
  async setPeekBounds(x: number, y: number, width: number, height: number): Promise<void> {
    return invoke('set_peek_bounds', { x, y, width, height })
  },

  /** Destroy the preview webview. */
  async closePeek(): Promise<void> {
    return invoke('close_peek')
  },

  /** Toggle reader mode inside the isolated preview. */
  async peekSetReader(enabled: boolean): Promise<void> {
    return invoke('peek_set_reader', { enabled })
  },

  // -- navigation safety interstitial ---------------------------------------

  /**
   * Proceed past the navigation safety interstitial: remember the host for the
   * session and drive the tab to the URL.
   */
  async navGuardProceed(tabId: TabId, url: string, host: string): Promise<void> {
    return invoke('nav_guard_proceed', { tabId, url, host })
  },
  async resolveMediaPermission(
    requestId: number,
    granted: boolean,
    remember: boolean,
  ): Promise<void> {
    return invoke('resolve_media_permission', { requestId, granted, remember })
  },

  // -- bookmarks (per-workspace, foldered) ----------------------------------

  async listBookmarkFolders(workspaceId: WorkspaceId): Promise<BookmarkFolder[]> {
    return invoke('list_bookmark_folders', { workspaceId })
  },

  async listBookmarks(workspaceId: WorkspaceId): Promise<Bookmark[]> {
    return invoke('list_bookmarks', { workspaceId })
  },

  async createBookmarkFolder(workspaceId: WorkspaceId, name: string): Promise<BookmarkFolder> {
    return invoke('create_bookmark_folder', { workspaceId, name })
  },

  async renameBookmarkFolder(id: number, name: string): Promise<void> {
    return invoke('rename_bookmark_folder', { id, name })
  },

  /** Reorder folders within a workspace: `ids` is the new order. */
  async reorderBookmarkFolders(ids: number[]): Promise<void> {
    return invoke('reorder_bookmark_folders', { ids })
  },

  async deleteBookmarkFolder(id: number): Promise<void> {
    return invoke('delete_bookmark_folder', { id })
  },

  async addBookmark(args: {
    workspaceId: WorkspaceId
    folderId: number | null
    url: string
    title: string
    faviconHash: string | null
    pinned: boolean
  }): Promise<Bookmark> {
    return invoke('add_bookmark', {
      workspaceId: args.workspaceId,
      folderId: args.folderId,
      url: args.url,
      title: args.title,
      faviconHash: args.faviconHash,
      pinned: args.pinned,
    })
  },

  async moveBookmark(id: number, folderId: number | null): Promise<void> {
    return invoke('move_bookmark', { id, folderId })
  },

  /** Pin or unpin a bookmark (independent of its folder). */
  async setBookmarkPinned(id: number, pinned: boolean): Promise<void> {
    return invoke('set_bookmark_pinned', { id, pinned })
  },

  /** Reorder the pinned quick-access row: `ids` is the new order. */
  async reorderPinnedBookmarks(ids: number[]): Promise<void> {
    return invoke('reorder_pinned_bookmarks', { ids })
  },

  /** Reorder bookmarks within a folder (or the unfiled list): `ids` = new order. */
  async reorderBookmarks(ids: number[]): Promise<void> {
    return invoke('reorder_bookmarks', { ids })
  },

  async deleteBookmark(id: number): Promise<void> {
    return invoke('delete_bookmark', { id })
  },

  // -- favicons -------------------------------------------------------------

  /**
   * Return a `data:` URL for the cached favicon, or `null` when the
   * hash isn't in the cache. The hash is the SHA-256 prefix of the
   * page's canonical origin — same algorithm as
   * `favicons::favicon_origin_hash`.
   */
  async getFaviconDataUrl(hash: string): Promise<string | null> {
    return invoke('get_favicon_data_url', { hash })
  },

  /**
   * Compute the origin hash for an arbitrary URL. Used by NewTabPage
   * quicklinks.
   */
  async faviconOriginHash(url: string): Promise<string | null> {
    return invoke('favicon_origin_hash', { url })
  },

  /**
   * Fetch + cache the favicon for `url` if it isn't already cached.
   * Used to pre-warm the launchpad quicklink icons on first boot.
   * Returns the origin hash on success, or null on parse / network
   * failure. Cache-hit fast path is a no-op.
   */
  async prefetchFavicon(url: string): Promise<string | null> {
    return invoke('prefetch_favicon', { url })
  },
} as const

export type Commands = typeof commands
