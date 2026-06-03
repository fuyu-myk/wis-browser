// Typed wrappers around `@tauri-apps/api/core` invoke() for every command
// exposed by `crates/shell/src-tauri/src/commands.rs`.
//
// Tauri 2 convention: JS-side arg keys are camelCase. The Rust-side function
// param `workspace_id: WorkspaceId` corresponds to `workspaceId` on the JS
// side. Passing snake_case from JS silently fails to deserialise.

import { invoke } from '@tauri-apps/api/core'
import type {
  AccentColor,
  BrowserState,
  Decision,
  PrivacyMode,
  RequestType,
  Tab,
  TabId,
  UserSettings,
  UserSettingsPatch,
  Workspace,
  WorkspaceId,
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

  // -- tab mutations --------------------------------------------------------

  async createTab(workspaceId: WorkspaceId, url: string): Promise<Tab> {
    return invoke('create_tab', { workspaceId, url })
  },

  async closeTab(id: TabId): Promise<void> {
    return invoke('close_tab', { id })
  },

  async reorderTabs(workspaceId: WorkspaceId, order: TabId[]): Promise<void> {
    return invoke('reorder_tabs', { workspaceId, order })
  },

  async pinTab(id: TabId, pinned: boolean): Promise<Tab> {
    return invoke('pin_tab', { id, pinned })
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
  }): Promise<void> {
    return invoke('create_tab_webview', args)
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

  /** History back in the tab. No-ops at start of history. */
  async tabGoBack(id: TabId): Promise<void> {
    return invoke('tab_go_back', { id })
  },

  /** History forward in the tab. No-ops at end of history. */
  async tabGoForward(id: TabId): Promise<void> {
    return invoke('tab_go_forward', { id })
  },

  // -- settings -------------------------------------------------------------

  async updateSettings(patch: UserSettingsPatch): Promise<UserSettings> {
    return invoke('update_settings', { patch })
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
   * Refresh the EasyPrivacy filter list from upstream and hot-swap it into
   * the running tracker engine. Returns the size of the new list in bytes.
   * Live tabs are re-attached with the new rules in the same call.
   */
  async refreshFilterLists(): Promise<number> {
    return invoke('refresh_filter_lists')
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

  /** Hide the overlay (kept around for the next show). */
  async hideOverlay(name: string): Promise<void> {
    return invoke('hide_overlay', { name })
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
