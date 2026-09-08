# Webviews, Windows & Overlays

This guide elaborates on WIS.md. If anything here conflicts, WIS.md wins.

## Compositing model

Native tab webviews (`tab-<uuid>`) composite **above** the chrome webview, so HTML chrome cannot draw on top of a page.

- Any floating UI over a page (find bar, palette, URL-bar suggestions, clear-data dialog, vault menu, pane-split menu, Peek, window picker) cannot be plain chrome DOM. It is a separate **overlay webview**, positioned natively.
- Stacking is tabs < widget panels < overlays. macOS uses `addSubview:` re-add order (`overlay.rs::promote_overlays_to_front`, `panels.rs::promote_panels_to_front`); Windows uses `win_webview2::promote_to_front` (`SetWindowPos(HWND_TOP)`), since HWND z-order is creation order and wry repositions with `SWP_NOZORDER`.
- Both promote helpers filter by the caller's **per-window** label prefix. Subview/HWND order is per `NSWindow`/frame, so restacking another window's still-loading views is pointless and crash-prone.
- Every webview is built with the wry drag-drop handler off: chrome via `dragDropEnabled: false` (`tauri.conf.json`) and `windows.rs`, overlays, tabs, widget panels and Peek via `disable_drag_drop_handler()`. Nothing in Wis consumes Tauri drag-drop events; wry still upgrades a None drag operation to Copy on macOS (cursor badge only). Hidden child webviews (idle overlays, background tabs, dismissed panels) are also parked off-window while hidden and moved back on show, keeping their logical bounds (eighth vendored wry patch), because AppKit resolves a drag destination from the frontmost view under the cursor without checking the hidden flag.

## Overlay webviews

Overlays are **per-window** and routed by URL, not label.

- Label is `overlay-<window>-<name>`, so each OS window owns its own find bar/palette. `overlay.rs::ensure_overlay` builds it from the calling window; `set_overlay_bounds` / `size_overlay` / `hide_overlay` resolve it the same way.
- `main.tsx` picks the surface from the **URL query** (`index.html?surface=<name>`), not the label, so the window prefix never affects routing. Chrome loads `index.html` with no query and renders the full App; `overlay/OverlayRoot.tsx` dispatches to a surface in `overlay/surfaces/`.
- A surface focuses itself with `focus_self` (calling webview becomes first responder, no label), so it works on any window.
- The `wis-asset` wallpaper protocol answers chrome surfaces only (`main`, `win-*`, any `overlay-*`), so tab/widget pages get 403.

## One window per workspace

The browser is **one OS window per workspace**; in-window multitasking is the pane split, never a second window over the same workspace (two windows would conflict its `tab-*` webviews).

- `windows.rs::spawn_window` builds a `win-<uuid>` `WebviewWindow` loading the same chrome bundle as `main`, binding its label to the workspace in `AppState::window_workspaces` **before** the build so the chrome's first `get_browser_state` resolves correctly. The bundle is window-agnostic: it reads its own label via `getCurrentWindow()` and routes tab/overlay/peek/widget creation to that window.
- **Invariant enforcement**: `set_active_workspace` and `new_window` resolve `window_for_workspace(id)` first; if another window already shows that workspace they **focus that window** (`focus_window`: deminiaturize + `set_focus`) instead of duplicating it. On collision `set_active_workspace` is a no-op and the caller's `invalidateQueries` refetch self-heals; `new_window` returns the owner's label.
- A secondary window's bind clears on the `Destroyed` event, freeing the workspace back into the picker.
- **New Window** opens the free-workspace picker (`WindowPickerSurface` via `useWindowPickerOverlay`), listing `free_workspaces` plus a *New empty workspace* row. **New Private Window** skips the picker and opens over a fresh ephemeral workspace.
- **Move to New Window** / **Move to workspace** (tab menu): `move_tab_to_new_window` creates a fresh workspace (private when the source is), recreates the tab atomically (`with_new_loose_tab` + `with_tab_closed`), then `spawn_window`s it. History/scroll survive the webview rebuild: the command captures the source webview's `interactionState` into `AppState::pending_restore_states` keyed by the new tab id, which `commands/webview.rs::create_tab_webview` replays.

## Session and geometry restore

The windowed set is persisted **continuously**, not at exit.

- Every `set_window_workspace` / `clear_window_workspace` writes the snapshot via `Storage::save_windowed_workspaces` (a `meta` key that, unlike document tables, survives `save_state`'s `clear_all`). Continuous writes survive a non-graceful exit; a `make dev` stop never fires `RunEvent::ExitRequested`.
- Startup seeds `window_workspaces` directly (not via the setter) so the set is intact when `main` calls `restore_session_windows` once (guarded by `AppState::session_restored`), which `spawn_window`s each saved workspace. Deleted/ephemeral workspaces resolve to nothing and are skipped, so private windows never restore.
- Geometry is persisted **per workspace, not per label** (`win-<uuid>` labels are fresh each run). `record_window_geometry` snapshots position/size on `Moved`/`Resized` into `AppState::window_geometry` under a `window_geometry` meta key, debounced by `geometry_gen`. `spawn_window` applies it at build time (no jump); `main` is declared `visible: false` and positioned-then-shown in `setup`.

## Main-thread AppKit marshaling

Every AppKit call reached from a command must run on the main thread; commands run on tokio workers and off-main AppKit crashes non-deterministically.

- The window is built (and its shadow disabled) via `run_on_main_thread`. This bites anything touching Cocoa directly rather than through a Tauri/`with_webview` handle (which already hops to the event loop). Example: `set_dock_icon`'s `setApplicationIconImage:` drives `-[NSDockTile display]` -> CoreGraphics, so it must be wrapped (a new private window calls it with `"closed"` on mount, else it segfaults on the worker).
- The Dock icon is process-global but the model is one workspace per window, so `useDockIcon` drives it only while its window holds focus.
- **App-wide broadcasts must be focus-scoped**: `wis-keyaction` and `wis-peek-open` use `app.emit`, which reaches every window and always matches a default-target listener, so ungated they would open a tab in every window at once. Each listener gates on `lib/windowFocus.ts::isWindowFocused()`. Same gotcha for `Webview.getAll()`: `useTabWebviews` cleanup filters by `wv.window.label` before closing leftover `tab-*` webviews, else an opening window would kill a sibling's live tabs.

## Engine-popup routing

Tab webviews route engine new-window requests by intent in `commands/webview.rs` `on_new_window`, delegating the discriminator to `popup_chrome.rs`.

- `wants_popup` is true when the request **declares geometry** (`window.open` with width/height/left/top, the OAuth-consent shape) or targets `about:blank`. That is **Allowed** as a native wry window using the opener's WKWebView config, sharing the workspace session + proxy and keeping the `window.opener` link OAuth needs.
- A plain navigation (`target=_blank`, featureless `window.open`) to the opener's **own site** (`same_site`, an eTLD+1 approximation with a small ccSLD table, so `mail.google.com` and `accounts.google.com` are one site) is Denied and navigated **in the current tab**. A **cross-site** plain navigation is Denied and re-emitted as `wis-open-in-tab-fg` (`link_open::OPEN_TAB_FOREGROUND_EVENT`), which the focused window opens as a foreground tab.
- Widget panels (`widget_panel.rs`) share this routing minus the in-place branch, and add a page-script rule for plain clicks: cross-site links post `wisOpenInTabFg` (foreground tab), same-site links stay in the panel. Security: the allowed popup is a bare wry webview with **no Tauri capability**, so the local-vs-remote boundary holds.

## macOS popup shims

The popup is built inside wry (no shell handle, vendor unpatched), so decorations are added app-side by `popup_chrome::ensure_running`, macOS-only.

- `window.close()`: wry gives the popup no `UIDelegate`, so WebKit's `webViewDidClose:` would be dropped. The poll assigns a `WisPopupUiDelegate` that closes a window only when the webview **is** the window's content view (the popup shape), so a browser window can never be closed by page script.
- File `<input>`: the same delegate implements `runOpenPanelWithParameters:`, so browse-from-computer in a popped-out compose window opens an `NSOpenPanel`. Without it WebKit no-ops the picker. Windows/Linux runtimes render the dialog themselves, so this shim is macOS-only.
- **Read-only address bar**: the decoration appears on the next main-loop tick (titlebar accessory with the window, no content jump), a read-only URL field plus scheme glyph (lock for https, warning for http), **seeded with the target URL** because the popup's `WKWebView` reports `about:blank` until it navigates. A refresh poll tracks later redirects and self-stops (`STOP_AFTER_MISSES`) once every popup closes.
- Detection is precise: child webviews (tabs, panels, overlays, Peek) are subviews of the main window, and every Tauri-managed window is excluded via `ns_window()`, leaving only engine `window.open` popups.

## Cmd / middle-click open-in-tab

Cmd/middle-click on a link must open a new **tab**, not a popup, so it is intercepted before `on_new_window` sees it.

- `link_open::page_script` intercepts `Cmd`/`Ctrl`+click and middle-click on `<a href>` (Opt+click stays Peek's), `preventDefault`s WebKit's open-in-new-window, and posts the href to the `wisOpenInTab` handler, which emits `wis-open-in-tab` (widget panels also bake it, plus `panel_page_script` posting cross-site plain clicks to `wisOpenInTabFg`; one `OpenInTabHandler` serves both names and picks the event from `message.name`). The focused window opens a **background** tab (`create_background_tab` -> `with_new_background_tab`), placed right after the opener (the active tab) and behind any tabs already opened from it, so a run of opens keeps link order (`with_new_related_tab`, which appends instead when the new tab files into a different folder than the active one). Foreground opens (`wis-open-in-tab-fg`) go through `create_related_tab` with the same placement, while Cmd+T, bookmarks and the launchpad keep appending. Real `window.open` / `target=_blank` (untouched by the script) still flow through `on_new_window`.
- **Chrome surfaces mirror the contract** (cross-platform, `onAuxClick` -> `create_background_tab`): launchpad tiles and history rows stay on the page; bookmarks-menu rows keep the menu up so several open in a row. Every chrome middle-click target also `preventDefault`s the middle-button **mousedown** (Chromium engages pan/autoscroll then, too early for `auxclick`-time preventDefault). Palette and URL-bar suggestion rows are excluded.

## Hover guard

An overlaying webview or floating window over a tab does not stop the covered tab `WKWebView` from updating CSS `:hover`, so `hover_guard.rs` suppresses it (macOS).

- Each covering surface carries a tracking area; while the cursor is over it, the covered window's tab pages get a scoped stylesheet disabling `pointer-events`, removed on leave.
- Scope: the widget panel (same-window child, `install_panel_hover_guard`), the video popout window (suppresses the origin window's other tabs), and engine `window.open` popups (free-floating, suppress **every** window's tabs via `arm_popup_suppression`).

## Capability boundary and window styling

The security boundary is **local vs remote content**, not just the window.

- The `default` capability has no `remote` block, so it applies only to local app webviews (chrome + `overlay-*`, loading bundled `index.html`) on its glob `["main", "win-*"]`. Tab pages and the Peek preview (`peek-page-<window>`) load remote `http(s)` and match no command-granting capability, with no exception.
- **Private tabs are not inspectable**. Webviews are built with `.devtools(!ephemeral)`. The Inspect row is omitted there as well.
- Chrome floats over an OS-vibrancy frame: `--content-bg` inside the periwinkle border (transparent in glass), darker `--chrome-bar` / `--chrome-bar-glass` for the frame, painted as a `box-shadow` spread on `.kb-content` (clipped by `.kb-main { overflow: hidden }`). Split panes reuse this as a cut-out: `.pane-surface` is a transparent rounded window whose **outset** `box-shadow` draws the pane ring then floods the outside with frame tone. An outset shadow never paints inside its box, so the page stays transparent. Off macOS the ring is **square** (`body:not([data-platform='macos'])`): native corner rounding (`set_webview_corner_radius`) is macOS-only, so a rounded ring would disconnect at a square WebView2 corner.
- Per-platform frame: macOS `titleBarStyle: Overlay` + `hiddenTitle: true` (native traffic lights), chrome shadow disabled to avoid a glass flash on navigation. Linux `decorations: false` + `transparent: true`, re-asserted post-realize for GNOME/Mutter CSD. Windows glass is **Mica** on Windows 11 (`windows::chrome_window_effects()` returns `WindowEffect::Mica`, applied to secondary windows at build and to `main` via `set_effects` before reveal, since `hudWindow` is macOS-only); Windows 10 gets none. Secondary windows mirror `main`'s glass via `chrome_window_effects()` plus the Overlay title bar and disabled shadow.
