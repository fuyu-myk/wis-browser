# Cross-Platform Layer & Page Channel

This guide elaborates on WIS.md. If anything here conflicts, WIS.md wins.

Wis is macOS-first (WKWebView). Windows (WebView2) is tier-1; Linux (WebKitGTK) is the newest tier, CI-compiled on ubuntu-22.04 but runtime-unverified. The seams below are where one shared page script or one shared frontend has to reach three different native webview engines. HOME/cache dirs go through the `dirs` crate, never raw `$HOME`/`%USERPROFILE%`.

## Page to chrome message channels

**The page scripts are identical across platforms; only the receiving plumbing differs per engine.** Each feature posts to `window.webkit.messageHandlers.<name>`.

- **macOS**: WKWebView gives each feature its own named `WKScriptMessageHandler`. The per-feature `install` fns register the real handlers here.
- **Windows**: WebView2 has one pipe (`window.chrome.webview.postMessage` to a single `add_WebMessageReceived`). `win_dispatch::SHIM_SCRIPT` (a document-start init script) recreates `window.webkit.messageHandlers` so each named `postMessage` forwards a `{ handler, body }` envelope; `win_dispatch::install` registers the one receiver that parses the envelope and calls the same per-feature logic (the per-feature `install` fns are no-ops here). Wired: `wisOpenInTab`, `wisNavGuard`, `wisPeek`, `wisMediaPermission`, `wisCapture`, `wisCosmetic`/`wisZap`.
- **Linux**: WebKitGTK exposes `window.webkit.messageHandlers.<name>` natively once `<name>` is registered on the tab webview's `UserContentManager`, so the shared scripts post unchanged with no shim. `gtk_dispatch::install` registers each named handler and routes to the same per-feature logic. Wired: `wisOpenInTab`, `wisNavGuard`, `wisPeek`, `wisMediaPermission`, `wisKeybind`, `wisCosmetic`/`wisZap`, `wisCapture`.

Also wired: `wisPageText` (all three platforms), the AI page-text channel (`page_text.rs`). Unlike the other channels it is pull, not push: the handler is installed on every tab but inert, and the `request_page_text(tab_id)` command evaluates the extraction script (the shared `reader::MAIN_CONTENT_FINDER` scoring plus a plain-text + 150k-char cap) on demand. The page posts `{ text, title }`; the handler reads the tab id and URL natively (from the webview's own label and `URL`, never from the page), re-caps the text in bytes, and emits it as the `wis-page-text` Tauri event. Only the main frame may answer: message handlers are exposed in every frame, but the URL reported belongs to the top-level document, so a subframe could otherwise put its own text forward as the page's. The event is a broadcast, so the chrome correlates a response to its request by tab id, which is why the id has to come from the label rather than the payload. Page text is untrusted downstream: it is never eval'd and the chrome renders it with React escaping only. The AI panel gates the send behind an explicit user action.

**Top-frame gating (Windows).** WebView2 runs every init script in subframes too (wry cannot restrict to the main frame) and the single dispatcher can only eval a reply into the main frame. So cosmetic/zap arms compare the message's `args.Source` to the webview's top-level URI (`win_dispatch::is_top_frame`) and drop iframe-originated posts; a zapped host is read from the webview URL, never the page. Cosmetic/zap are also gated to non-Permissive workspaces, and `wisCapture` gates off for ephemeral tabs so private logins are never captured.

## Keyboard dispatch

**Accelerators are authored once in macOS form (`Cmd+T`) and localized to the host; dispatch mechanism then diverges per engine.** Source of truth is Rust: `keybinding::default_keymap()` delegates to `default_keymap_for(cfg!(target_os = "macos"))`, swapping `Cmd` to `Ctrl` on Windows/Linux (muda would otherwise bind literal `Cmd` to the Windows/Super key). The frontend keymap is display-only: `lib/platform.ts::isMacOS()` drives `lib/keybindings.ts::localizeAccelerator`/`prettyAccelerator`.

- **macOS**: the native global menu is the real keystroke handler.
- **Windows**: native accelerators never fire (a focused WebView2 handles input out-of-process, bypassing Tauri's `TranslateAcceleratorW` hook). Shortcuts dispatch in JS per focus location: chrome and every overlay webview mount `useKeybindCapture` (`Window.tsx` with `takeFocusOnMount`, `OverlayRoot` without); every tab webview gets an injected forwarder (`keybind.rs`).
- **Linux**: the GTK accel-group handles chrome focus, but a focused WebKitGTK tab webview swallows key events, so tab webviews also take the injected forwarder.

**The tab forwarder is two-tier** (`keybind::KEYBIND_JS`): a chrome-owned `RESERVED_ACTIONS` set (tab/window lifecycle + address-bar focus) is stolen at capture phase; everything else is page-first, dispatched from a bubble-phase listener only when the page did not `preventDefault` (so Ctrl+S in a web editor saves the document). Matched ids post over `wisKeybind`, provenance-gated: the posting webview must be a workspace's current active tab, and the re-emitted `wis-keyaction` is stamped with the source tab, which `useKeyActions` requires to belong to its own window's workspace. Rebinds push live via `keybind::apply_to_all_tabs` (top-level documents only). `normalizeAccelerator` (both `lib/keybindings.ts` and the `KEYBIND_JS` mirror) must stay in lockstep with `keybinding.rs::default_keymap_for`, since both now drive live handling, not just display.

**Menu refresh.** A live rebind rebuilds the menu via `apply_menu`, which must dispatch to the main thread (commands run on tokio workers; off-main `NSMenu` mutation silently fails) and call `Menu::set_as_app_menu` on macOS (plain `set_menu` does not refresh live key equivalents). On Windows/Linux the menu is installed but its window bar is hidden (`app.hide_menu` after each `set_menu`, and `spawn_window` hides secondary windows' bars); Wis drives every command from chrome UI.

**Startup focus (Windows).** tao's `is_focused()` reads the top-level HWND flag (false whenever a WebView2 child holds focus), poisoning `isWindowFocused()` and leaving keybinds dead until a click. Root cause: Tauri's multiwebview mode makes even the chrome a wry child webview, and wry only installs its frame `WM_SETFOCUS`-to-`MoveFocus` subclass for non-child webviews, so focus landing on the bare frame HWND (boot, alt-tab, a closing webview) stranded keystrokes. `win_focus::install` (for `main` and every `spawn_window`) restores the missing subclass: on frame `WM_SETFOCUS` it forwards focus to the active tab webview, else the chrome. Supporting fixes: `windowFocus.ts` ORs `document.hasFocus()` into the boot correction; `focus_self` gained a Windows arm; `close_peek` and the find bar hand focus back via `win_focus::restore_webview_focus`/`focus_webview`.

## WebView2 environment gotchas (Windows)

**WebView2 runs one browser process per user-data folder with browser arguments baked in at launch, so folder sharing must be exact.**

- Any builder that sets `proxy_url` (wry appends `--proxy-server`) must also set `data_directory`, or it lands in the shared default folder whose already-running proxy-less process makes creation fail with `ERROR_INVALID_STATE`. Wired: tab webviews, widget panels, ephemeral tabs and Peek (per-workspace `workspace_webview_data_dir` / `ephemeral_webview_data_dir`).
- `scroll_bar_style` is likewise an environment option and must match across every webview in a folder: content webviews (tab/peek/widget) set `ScrollBarStyle::FluentOverlay`; chrome and overlays keep the default in their own folder.
- Vendored wry destroys the container HWND on any creation failure (see `vendor/wry/WIS-PATCH.md`): the leak was a `WS_VISIBLE`, nothing-painting, hit-testable child window that swallowed mouse input (a failed Peek left the page center click-dead).

## Native stacking, rounding, input guards (Windows)

**HWND z-order is creation order and wry repositions with `SWP_NOZORDER`, so macOS `addSubview:` re-adds need explicit Windows analogues.**

- `promote_overlays_to_front`/`promote_panels_to_front` gain `win_webview2::promote_to_front` (`SetWindowPos(HWND_TOP)`), keeping tabs < panels < overlays. Because promote loops walk `app.webviews()` (a `HashMap`, arbitrary order), `show_widget_panel`/`open_widget_panel` re-promote the specific shown panel after the loop.
- Corner rounding uses `SetWindowRgn` + `CreateRoundRectRgn` on the container HWND (`win_webview2::set_corner_radius`); a GDI region is fixed-size, so the radius re-cuts per label on every resize (`set_tab_webview_bounds`/`set_peek_bounds`). The split-pane CSS squaring override is Linux-only (`body[data-platform='linux']`) since Windows panes round natively.
- A WebView2 child HWND hit-tests regardless of transparency and can capture mouse input while hidden, unlike an empty WKWebView. Guards: hidden tab webviews are parked offscreen on hide (`PARK_HIDDEN_OFFSCREEN` in `useTabWebviews`); `useUrlbarSuggest.close` supersedes any in-flight `refresh` whose late `showUnderBar` would re-show a closed overlay over the content column.

## Native UX ports

**Several AppKit features have Windows/Linux equivalents in the same per-feature module.**

- Clipboard copy (`clipboard.rs`): `NSPasteboard` on macOS, `arboard` on Windows/Linux, text-only.
- File dialogs (`panels.rs`): `NSSave/OpenPanel` on macOS, `rfd` on Windows/Linux, all shown on the main thread via `dispatch`; the ask-where save panel and wallpaper picker call `rfd` directly (already inside a main-thread callback, so no re-`dispatch`).
- Download open/reveal (`downloads.rs`): `open`/`open -R` (macOS), `xdg-open` (Linux), `explorer`/`explorer /select,` (Windows).
- Split-pane click-to-focus (`pane_focus.rs`): app-wide `NSEvent` monitor / per-controller `add_GotFocus` gated on a live `GetKeyState(VK_LBUTTON)` press.
- Find-in-page (`find.rs`): WebKit find SPI (`_WKFindDelegate`, retained as an associated object; reports total + index over `wis-find-result`) on macOS; a JS `CSS.highlights` engine over `wisFindResult` on Windows (top-document, ASCII-only case folding to keep offsets length-stable); WebKitGTK `FindController` on Linux, which exposes no current-match index so it reports the total count only.
- Print / Save-as-PDF (`print.rs`): WKWebView `printOperationWithPrintInfo:` / `ICoreWebView2_16::ShowPrintUI` / WebKitGTK `webkit_print_operation_run_dialog`.
- Open-in-default-browser (`external.rs`) is already cross-platform via the OS shell.
- Default-browser registration + external link intake (`default_browser.rs`): macOS declares http(s) in `Info.plist` (`CFBundleURLTypes` + `CFBundleDocumentTypes`) and asks via `NSWorkspace setDefaultApplicationAtURL:` on the main thread (`LSSetDefaultHandlerForURLScheme` fallback for pre-12), links arriving as `RunEvent::Opened`; Windows writes the HKCU `StartMenuInternet` registration (`win_default_browser.rs`) and opens `ms-settings:defaultapps` since programmatic switching is not allowed, links arriving via argv with `tauri-plugin-single-instance` forwarding second launches; Linux ships a custom bundler desktop template (`wis.desktop`: `MimeType` + `Exec %U`, reused by deb and AppImage) and applies via `xdg-settings`, links via argv likewise. Incoming URLs pass a pure http(s)-only validator, queue in a module static (not `AppState`: the launch Apple Event can precede tauri's `setup`, when managed state does not exist yet and a `state()` call would panic-abort across the extern "C" delegate) until the main chrome's one-shot `drain_external_urls`, then stream as `wis-open-external` events (main-window handler, deliberately not focus-gated: an external open can precede app activation).

## Feature-availability toast

**A user-invokable feature with no working backend on the running platform raises a general `overlay-toast` notice instead of a silent no-op or raw error.** `lib/platformSupport.ts` is the single source of truth (`FEATURES`: feature id to `unavailableOn` platforms, reflecting the current build). `guardFeatureUnavailable` short-circuits `useKeyActions` dispatch (covers keybind and palette, since the palette re-emits `wis-keyaction`) and the video-popout controller (only an explicit request, never the automatic tab-hidden pop-out). Wiring a feature up on a platform means deleting its entry so the toast stops. Currently toasted: visible/region/window capture and video pop-out on Linux.

## Deferred to a Windows runner

**These have no compile-only path; each needs runtime validation on real hardware.** Session-cookie restore (Tauri's cookie API needs a live webview, but restore must precede the first load), uBO `+js` scriptlet swap (document-start registration inside the navigation event, re-entrant message-pump risk), full session/history restore (no `interactionState` equivalent), and the whole-window composite screenshot (needs the chrome+overlay compositor, so Window degrades to a page-only shot). Windows page capture (`CapturePreview`) is done, so Visible works, Window falls back to it, and Region crops it (`crop_png`); a failed clipboard copy is fatal (no false "Copied" toast). Per-tab task-manager memory is an honest empty on Windows (WebView2 shares one process group with no per-tab renderer pid). Default-browser registration compiles on CI, but the Settings > Default apps listing and argv link forwarding need a real runner.

## Platform status

| Feature | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Page to chrome channel | native handlers | shim + single pipe | native, no shim (CI-only) |
| Keyboard dispatch | global menu | JS (chrome + tab forwarder) | GTK accel + tab forwarder (CI-only) |
| Vault biometric gate | Touch ID | Windows Hello | not yet |
| Glass / window styling | vibrancy | Mica | not yet |
| Clipboard / zoom / mute / print / downloads / wallpaper | yes | yes | yes (CI-only) |
| Find-in-page | count + index | count + index | count only |
| Screenshots | visible/region/window/composite | visible/region/window (page fallback) | none |
| Video pop-out | reparent to float | native PiP | none |
| Session + history restore | yes | deferred to runner | not yet |
| Session-cookie persistence | yes | deferred to runner | not yet |
| Per-tab task-manager memory | yes | empty (shared process) | not yet |
| Default-browser registration + external links | yes | deferred to runner | CI-only |

Linux rows marked CI-only compile on ubuntu-22.04 but have no hardware run yet. WIS.md carries the authoritative summary in its Cross-platform conventions and Known limitations sections.
