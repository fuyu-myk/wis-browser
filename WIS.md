# WIS.md

Wis loads `WIS.md` from the workspace root as agent memory (similar to AGENTS.md / CLAUDE.md). This file is also the project's living architecture doc.

**Read it before making changes, and update it whenever you change architecture, add a crate/module, or ship a user-facing feature.** Keeping this file accurate is part of "done" for any change.

## Project

**Wis**: open-source, fast, safe and lightweight browser that draws inspiration from browsers like Zen and Firefox. macOS-first; Windows is a growing tier-1 port (vault via DPAPI + Windows Hello, Mica glass, and a widening set of webview features reached through the `win_webview2` WebView2 helper, see Cross-platform conventions). Linux (WebKitGTK) is the next port: its page -> chrome layer (`gtk_dispatch`), a widening set of features (cosmetic filtering + element picker, find-in-page, keyboard shortcuts, camera/mic consent, peek, nav-guard, save-password capture), and the multi-webview compositing/overlay/focus stack (vendored `tao`/`tauri-runtime-wry`/`wry` patches) are wired, compile on CI (ubuntu-22.04), and the **core flows are runtime-verified in an X11 VM**; glass/transparency rendering fidelity and a few per-feature checks await real hardware (`docs/contributing/linux-vm-testing.md`). A few native pieces (session/history restore, session-cookie persistence, per-tab memory) remain macOS-only today.

- Bundle id: `app.myk.wis`
- Package manager: **pnpm**
- Run/build: use the `Makefile` (`make dev`, `make build`, `make check`, `make test`, `make lint`, `make fmt`). `make dev` runs `pnpm tauri dev`; the built-in Tor client is a **default cargo feature**, so a plain `make dev` already includes it.
- Frontend check: `cd crates/shell && pnpm build` (= `tsc && vite build`; this is the typecheck gate, there is no separate test script yet).
- Rust checks: `make lint` (`cargo clippy --workspace --all-targets -- -D warnings`) and `make test` (`cargo test --workspace --all-targets`).

## Quality bar

Production-grade or it does not ship. Every change is judged against all of these, not just "it works":

- **Correctness**: edge cases, failure modes, concurrent access. No "works for now".
- **Performance**: ultra-lightweight is the product. For every change ask: how much RAM it costs, whether it adds IPC round-trips or redundant requests, whether it triggers extra re-renders or wasted work, whether it pulls a heavy dependency. Unused features consume zero resources (gate heavy deps behind cargo features, start subsystems lazily).
- **Security**: no critical security holes. Validate at every boundary (IPC, fs, network). Secrets are never logged. Page webviews (`tab-<uuid>`) are never granted Tauri capabilities.
- **UI/UX**: polished, professional, premium. Every state and detail considered.
- **Architecture**: new or changed logic lives in pure, dependency-light functions (functional core); tauri commands and React components stay thin (imperative shell). Keeps it testable without a later rewrite.

Verify before claiming done: `pnpm build` (frontend), `make lint`, `make test`. A change to a core subsystem (main browser, storage, network) needs a test that locks the invariant.

## Conventions

- **Comments**: default to none, the code should explain itself. If genuinely needed, 1-2 lines on *why*, never *what*. No AI-generic filler.
  - Avoid the use of "we" in comments, be explicit and direct: "this runs on startup" instead of "we run this on startup".
- **No em-dash** anywhere: code, comments, commits, docs.
- **No emojis** anywhere.
- **Imports**: always `@/...` on the frontend, never relative across modules.
- **pnpm only**, never npm/npx/yarn.
- **References to plans**: references to phases in the implementation plan/roadmap should be avoided in code/comments/docs.
- **Immutability**: every state transition returns a new value; never mutate in place. (`Tab` is `Clone + PartialEq` but not `Eq` because it carries an `f64` zoom.)

## Architecture

This section orients; the deep detail lives under `docs/architecture/` (see Further reading). Those docs elaborate on WIS.md and WIS.md wins on any conflict.

### Crate layers

**Cargo workspace, five crates; dependencies flow strictly downward** (upper layers import lower, never the reverse).

```text
wis-browser-app  (crates/shell/src-tauri)  - Tauri shell, IPC commands, AppState, native macOS glue
       |
  wis-network    (crates/network)           - privacy decision engine, proxy, DoH, Tor
       |
  wis-storage    (crates/storage)           - Storage trait + SQLite implementation
       |                 \
   wis-core      (crates/core)   wis-secrets (crates/secrets) - vault crypto + keychain
   (pure data model, no I/O)
```

- **`wis-core`**: pure, dependency-light data model. `BrowserState` root aggregate, `Workspace`, `Tab`, `UserSettings`, typed IDs, and pure `with_*` builders. Modules include `state`, `workspace`, `tab`, `tab_folder`, `settings`, `layout`, `pane`, `privacy`, `proxy`, `search`, `keybinding`, `link_safety`, `netscape`, `new_tab`, `password_csv`, `user_agent`, `download`, `background`, `icon`, `theme`, `backup`, `design`, `widget`, `userscript`. Detail: `docs/architecture/data-model-and-state.md`.
- **`wis-secrets`**: vault cryptography and master-key storage, isolated. ChaCha20-Poly1305 AEAD under a CSPRNG master key held in the OS secure store (macOS Keychain / Windows DPAPI blob), released after a biometric check and zeroized on drop. Detail: `docs/architecture/security-vault-and-permissions.md`.
- **`wis-storage`**: persistence. `Storage` trait + sole `SqliteStorage` impl (bundled SQLite, WAL, `synchronous=NORMAL`); normalized tables (not a JSON blob), append-only migrations `M0001`..`M0018`, `VACUUM INTO` backup + resilient open. Detail: `docs/architecture/data-model-and-state.md`.
- **`wis-network`**: privacy decision engine. `TrackerBlocker` (adblock crate, disk-memoized engine), `content_filter` (`WKContentRuleList`), `HttpsUpgrader`, `DohResolver`, `RequestPolicy`, per-workspace loopback proxy (SOCKS5 / built-in Tor via `arti-client`, `tor` feature default-on). Detail: `docs/architecture/privacy-and-network-engine.md`.
- **`wis-browser-app`** (`shell/src-tauri`): Tauri wiring. `AppState` (browser mutex, per-window active workspace, storage, blocker, resolver, proxies, vault key, screenshots, Tor); native modules for IPC `commands`, `menu`, `overlay`, `find`, `cookies`, `biometrics`, `vault`, `capture`, `peek`, `reader`, `screenshot`, `backup`, `permissions`, `video_popout`, `audio`, `load_progress`, `tab_preview`, and the `win_*`/`gtk_dispatch` cross-platform seams. Detail: split across the docs below.

### IPC command pattern

**Every mutating command goes through `mutate`: lock state, snapshot for rollback, compute the next state with a pure `wis-core` `with_*` transition, persist, revert on persist failure.** After success it emits `browser-state-changed`; the frontend refetches via React-Query (the single JS source of truth). Read-only commands hold the lock only long enough to clone a slice. Detail: `docs/architecture/data-model-and-state.md`.

### Webview compositing, overlays, windows

**Native tab webviews (`tab-<uuid>`) composite above the chrome webview, so any floating UI is a separate per-window overlay webview.** The browser is one OS window per workspace (`windows.rs`); multi-window session/geometry restore, engine-popup routing (`popup_chrome`, `on_new_window`: geometry popups stay native, plain navigations become tabs), and Cmd/middle-click open-in-tab all live here. Off-main AppKit is marshaled via `run_on_main_thread`. Every webview, chrome and page alike, disables wry's drag-drop handler so pages keep native HTML5 DnD and file drops. Detail: `docs/architecture/webviews-windows-and-overlays.md`.

### Capability boundary

**The security boundary is local vs remote content, not just the window.** The `default` capability grants commands only to local app webviews (chrome + `overlay-*`) on the `["main", "win-*"]` glob; tab pages and the Peek preview load remote URLs and match no command-granting capability, so they can never call a command (sole exception: `tab-devtools.json` grants only the devtools toggle). Detail: `docs/architecture/security-vault-and-permissions.md`.

### Frontend (React / TypeScript)

**Lives in `crates/shell/src/`; imports use `@/...` aliases, relative cross-module imports forbidden.** Thin imperative shell over the `wis-core` model: `components/` (chrome, tab strips, settings, pages), `hooks/` (`useBrowserState`, `useTabWebviews`, overlay hooks), `lib/` (`keybindings`, `urlbarSuggest`, `theme`, motion), `bindings/` (typed IPC wrappers + `wis-core`-mirroring types, camelCase to serde at the boundary). Detail: `docs/architecture/chrome-shell-and-layouts.md`.

### Window styling

**Chrome surfaces use layered background tiers and a frame `box-shadow` for contrast; glass is OS vibrancy (macOS), Mica (Windows 11), none elsewhere; the glass tint is opacity-compensated per platform (`body[data-platform]` overrides) since Mica and the bare Linux desktop show through far more than macOS vibrancy.** The native tab webview's corner radius follows the Corners design setting (`--radius-scale`, re-applied live on change). Split panes are transparent rounded cut-outs with an accent ring (square off macOS); chrome scrollbars invert vs the theme; macOS uses `titleBarStyle: Overlay`, Linux `decorations:false` + `transparent`. Linux pins corners to square (`--radius-scale: 0`, the Design > Corners control locked to Sharp) because its child webviews render with square corners; it also builds no native menu bar (keybinds are JS-dispatched). Detail: `docs/architecture/webviews-windows-and-overlays.md`.

### Cross-platform layer

**A per-feature seam ports each macOS-AppKit capability to Windows/Linux, verified per platform.** The page -> chrome channel is `WKScriptMessageHandler` (macOS) / a single `win_dispatch` receiver behind a shim (Windows) / native `gtk_dispatch` handlers (Linux); keyboard shortcuts are native menu accelerators on macOS but JS-dispatched on Windows and Linux (native accels never fire behind a focused webview; Linux attaches no native menu at all, chrome-side capture + the in-page forwarder cover both focus states). Native UX ports cover clipboard, file panels (`rfd`), downloads shell-out, click-to-focus (`focus_self`/`focus_webview` per platform), find, and capture; a **feature-availability toast** replaces silent no-ops. On Linux, positioned child webviews (tabs, overlays, peek) required a vendored `tao`/`tauri-runtime-wry`/`wry` patch set: a full-window `gtk::Fixed` overlay for compositing (upstream tauri#10420) plus show/map lifecycle fixes (WebKitGTK neither repaints a resize applied while unmapped nor raises on show). Linux is **compile-verified on CI (ubuntu-22.04) and core-flow runtime-verified in an X11 VM**; rendering fidelity pends real hardware. Detail: `docs/architecture/cross-platform-and-page-channel.md`.

## Features

Each group maps to a `docs/architecture/` doc that carries the mechanism details; the one-liners below orient.

### Privacy & network (`docs/architecture/privacy-and-network-engine.md`)

- **Network privacy**: per-request DoH with a provider picker (Cloudflare, Quad9, Google; applied live, no restart), HTTPS-First, host-level + `WKContentRuleList` tracker blocking, and a per-workspace upstream proxy (built-in Tor, or a SOCKS5 host and port entered in settings and validated at the boundary, both reachability-probed).
- **Cosmetic filtering + element picker / zapper**: hides empty ad slots / nags beyond network blocking (`wisCosmetic`/`wisZap`), `:has(...)` procedural rules included; per-host user rules persist. Runs on macOS/Windows/Linux (Linux via `gtk_dispatch`).
- **Scriptlets & user scripts**: uBO `+js` (Brave resource library) and Tampermonkey `.user.js` injected per-navigation (page world, no `GM_*`); the scriptlet/user-script swap is macOS-only for now.
- **Filter lists**: EasyList/EasyPrivacy plus user subscriptions feed the engine (YouTube ad blocking via uBO scriptlets); managed in Settings -> Privacy.
- **Tor VPN**: built-in Arti with a simple on/off toggle; re-routes by reloading workspace tabs.
- **Fraudulent website warning**: WebKit Safe Browsing enabled on every remote webview (Settings toggle; macOS-only, the other engines expose no equivalent and their settings row says so; navguard covers risky links everywhere).

### Security & data protection (`docs/architecture/security-vault-and-permissions.md`)

- **Password vault**: ChaCha20-Poly1305 at rest, master key in the OS key store gated by a biometric (Touch ID / Windows Hello); save-capture, saved-account picker, origin-matched autofill (app -> page eval only); CSV import/export (`wis_core::password_csv`, biometric-confirmed both ways, plaintext-export warning).
- **Stay logged in across restarts**: encrypted session-cookie snapshot restored before first load, fixed 14-day TTL, gated by the `persist_sessions` setting; never for ephemeral workspaces.
- **Camera & microphone consent**: app-side `getUserMedia` gate + in-browser prompt with remembered decisions (macOS/Windows/Linux; the Linux prompt is wired but not yet exercised in the VM).
- **Navigation safety interstitial**: `navguard` re-assesses risky links (click-time and page-load fallback) and interrupts High/Medium threats; shares `link_safety` heuristics with Peek.
- **Open in app**: custom-scheme navigations (zoommtg://, slack://, mailto:) are cancelled at every policy seam (tabs, new-window, macOS engine popups, Peek, widget panels) and offered as an OS handoff via a prompt overlay with per-site remembered decisions (`app_launch`, `UserSettings.app_launch_permissions`); this is what completes OAuth "open in app" linkbacks. Windows/Linux popup redirects fall back to engine defaults.
- **Strict CSP** on local surfaces and the local-vs-remote **capability isolation** above; the vendored Tauri-stack patches (`wry`/`tao`/`tauri-runtime-wry`) stay clean verbatim drops of upstream PRs, dropped when those merge; feature code never patches them.

### State & storage (`docs/architecture/data-model-and-state.md`)

- **Immutable state, normalized persistence**: `BrowserState` saved across normalized SQLite tables in one transaction, coalesced debounced writes with immediate destructive writes, WAL + `VACUUM INTO` backup resilience.
- **Encrypted profile backup**: passphrase-sealed `.wisbackup` (Argon2id + ChaCha20-Poly1305), merge-on-import with id re-mint; passwords opt-in via the device vault key.

### Chrome shell & layouts (`docs/architecture/chrome-shell-and-layouts.md`)

- **Workspaces**: color-accented profiles, each with its own WebView data store, proxy, and privacy mode; **private browsing** is an ephemeral workspace (excluded from save, wiped on close).
- **Multi-window**: one OS window per workspace, free-workspace picker, move-tab-to-new-window/workspace, session + geometry restore.
- **Split panes**: per-workspace binary `PaneNode` tree, resizable, focused-leaf highlight, native click-to-focus.
- **Pinned tabs & tab folders**: docked quick-access pins; per-workspace named/recolorable/icon-bearing folders of live tabs, rendered as a sidebar tree or top-tabs islands, with drag-reorder and selection-targeted new tabs.
- **Folder + workspace icons**: glyph/emoji icons via `wis_core::icon`, shared `IconPicker`.
- **Theming & motion**: custom `.wistheme` themes, per-workspace gradient/image/GIF backgrounds, Light/Dark/System mode pushed to web content, and a tokenized three-tier motion system (drag choreography, FLIP close glides, pane/widget entrances).
- **AI assistant (bring your own key or run it locally)**: a docked chrome-side panel for free chat, page summaries, and questions about the open page, against Anthropic or any OpenAI-compatible endpoint (Ollama, LM Studio, OpenRouter). Dormant by default: it activates on a key sealed in the OS key store, or on an OpenAI-compatible base URL (a local server authenticates nothing, so the key is optional there and the bearer header is omitted entirely). Until one of those is set there is no entry point and no code path, and none at all on Linux, which has no key store. Requests route through the workspace proxy, except a loopback endpoint, which is dialled direct so a local model's traffic never leaves the device; the transcript is in-memory for the window, dropped on workspace change and never persisted; page text is shared only by an explicit action, per send, and every turn that carried it says so. A reasoning model's working streams into its own collapsible disclosure (labelled with how long it thought) and is never replayed as conversation history; answers render as markdown via a pure subset parser mapped to escaped React elements, never injected HTML, with links opening as new tabs behind an http(s) allowlist. An opt-in **web access** toggle (OpenAI-compatible providers; runtime-verified on macOS, wired for Windows and Linux via their message dispatchers) lets the model call `web_search` and `web_fetch`, so a local model can pull current answers: pages load in a hidden incognito webview with no Tauri capabilities, routed through the workspace proxy, and every URL, entry and redirect alike, must pass a public-web SSRF gate (`is_public_web_url`), with fetches further allowlisted to search results and user-given URLs; each round shows as an expandable "using tool" indicator that settles into what it found, capped at four rounds, and Answer now breaks a failing tool loop.
- **URL-bar suggestions** and a **configurable launchpad + homepage** (editable shortcut tiles, boundary-sanitized).
- **Custom search engines**: user-defined engines (`wis_core::search`, `{searchTerms}`/`%s` templates, boundary-sanitized) with address-bar keyword triggers (`kw query`); built-in engines can carry optional keywords too (custom wins on collision); any engine can be the default (Settings > Search), resolved consistently across URL bar, launchpad, and palette.

### Tabs, panels & media (`docs/architecture/tabs-panels-and-media.md`)

- **Tab memory & sessions**: idle-sleep + lazy restore with `interactionState` wake, full session restore across restart (`tab_sessions`, `M0018`), and a per-webview **task manager** (macOS; empty off macOS).
- **Quick browser features**: per-tab zoom, find-in-page with match count (Linux count-only), numbered/next/prev tab switching, reopen-closed-tab, hard reload, print/Save-as-PDF, and configurable rebindable shortcuts.
- **Load-failure error pages**: a failed navigation (DNS, connection refused, timeout, TLS) renders a chrome-side error page with retry in the tab's place (webview hidden but alive); `wis_core::LoadError` normalizes each engine's native code. macOS surfaces the failure via a vendored-wry side channel (a sixth patch, since `PageLoadEvent` cannot gain a Failed variant without vendoring tauri too), Windows via `add_NavigationCompleted`, Linux via WebKitGTK `load-failed` (kind is coarse there).
- **Peek**: Opt+click isolated incognito link preview with a safety banner and reader toggle.
- **Reader mode**: in-place clean-article transform shared with Peek.
- **Video popout**: always-on-top mini player (the tab's own reparented webview on macOS; native Picture-in-Picture on Windows; toasts on Linux), automatic on tab-hide and manual.
- **Tab audio**: per-tab audibility indicator + engine-level mute (macOS/Windows/Linux).
- **Page capture**: clipboard-default screenshots (visible / region / whole-window composite) with a Save toast; window composite is macOS-only (page-only fallback elsewhere).
- **Downloads**: safety-first `~/Downloads` handling (sanitized names, quarantine xattr, no overwrite), persisted history (`M0014`), ask-where option, popup + full page; a navigation that becomes a download leaves the tab on its current page, and a tab opened only for a download closes itself.
- **Sidebar quick-launch widgets**: a curated catalog (social, AI, productivity) plus user-added custom-URL sites placeable in any dock row (picker at add time, or drag across rows; curated widgets stay in their own row), validated at the IPC boundary (`Widget::validate`: http(s) + bounded label + group override customs-only); per-widget floating panels sharing the workspace store/proxy, with LRU count + memory caps, keep-loaded/idle-sleep, and a header bar. Every remote-webview destroy path (widgets, tabs, peek, window close) closes the WebKit page first via `webview_close`, which makes the WebContent process exit deterministically; panel creation is serialized against a WebView2 message-pump deadlock.
- **Bookmarks**: per-workspace foldered + independently pinnable quick-access, star flow, drag organize (SQLite, never in `BrowserState`); HTML (Netscape) import/export via Settings > Bookmarks (`wis_core::netscape`, duplicates skipped, nested folders flattened).
- **History & cookies**: per-workspace deduped history with search; native site-data listing + per-domain/full clear.
- **Browser identity**: modern Safari-class UA by default with per-host overrides (`wis_core::user_agent`), OS-following platform token; **open in default browser** exit hatch.
- **Default browser**: registrable as the OS default on all three platforms (Compatibility settings row; macOS system prompt, Windows Default Apps hand-off, Linux `xdg-settings`); links clicked in other apps open as tabs, queued until the chrome is ready (`default_browser.rs`).

### Cross-platform & page channel (`docs/architecture/cross-platform-and-page-channel.md`)

- **Page -> chrome channels**, **keyboard dispatch** per platform, **WebView2 environment** gotchas, native **stacking/rounding/input** guards, native **UX ports**, the **feature-availability toast**, and the current **platform-status matrix** (Linux compile-verified on CI, core flows VM-verified).

## Known limitations / engine tradeoffs

Wis renders with the OS system WebView (WKWebView on macOS) via Tauri/wry, the same engine as Safari. That choice is the source of the product's wins (tiny bundle, low RAM, Safari-class speed and battery, no bundled engine to update) and dictates these limits, which are structural rather than bugs:

- **Premium DRM video (Widevine)**: WKWebView's only CDM is Apple FairPlay, not Google Widevine, and there is no API to inject a Widevine CDM (the CDM hooks the engine's media pipeline, which WKWebView does not expose). FairPlay itself is not licensed for general third-party-app playback, and services like Netflix gate on the Safari app. Result: premium DRM streaming (Netflix/Disney+/etc., especially 1080p/4K) is degraded or blocked. Mitigation is the **Open in default browser** action, not an in-app fix. Adding native Widevine would require switching to a Chromium engine (CEF) plus a Widevine license, which would forfeit the bundle/RAM/battery wins; explicitly not pursued. Note: even Firefox forks like Zen punt DRM to Firefox for the same licensing reason. **MSE + EME/FairPlay**: Wis does enable WebKit's Media Source + Encrypted Media Extensions on remote webviews (`webview_prefs::apply_media_extensions`, guarded SPI `_setMediaSourceEnabled:`/`_setEncryptedMediaAPIEnabled:`/`_setLegacyEncryptedMediaAPIEnabled:`; `UserSettings.media_extensions_enabled`, default on, toggle in Settings -> Privacy). MSE alone (off by default in embedded WKWebView) restores adaptive streaming on many non-DRM video sites; EME exposes the FairPlay key system to services that request it. This does not unlock Widevine-gated services, and Safari-app-gated FairPlay services (Netflix) still refuse, those remain the Open-in-Safari handoff.
- **Browser extensions**: an embedded WKWebView cannot load Safari Web Extensions (reserved for Safari). Wis substitutes a native blocking stack (`WKContentRuleList` + the `adblock` crate, with cosmetic filtering, scriptlets and an element picker) plus a Tampermonkey-style user-script runner, rather than a WebExtensions runtime.
- **Site isolation**: WebKit's site isolation is weaker than Chrome's (still rolling out upstream), offset by strong OS sandboxing and out-of-process rendering.
- **Other**: no cross-device sync, no mobile companion, and some newer web APIs (WebUSB/WebBluetooth/web push/PWA install) are partial or absent under WebKit. Some native parity still lags off macOS: session/history + session-cookie restore, per-tab task-manager memory, and the vault biometric gate remain macOS-only. Linux's remaining gaps beyond those: per-site data management (list/delete/clear render an honest "not available yet"), uBO `+js` scriptlet injection, the theme -> page color-scheme push, and the media-consent gate on widget panels and Peek (tabs are gated); each degrades visibly or deny-safe, never silently.

## Build & release

- **CI** (`.github/workflows/ci.yml`): on push / PR to `main`, a matrix over macOS / Windows / Ubuntu installs deps (Linux pulls `libwebkit2gtk-4.1-dev` + packaging tools), builds the frontend, then runs `cargo fmt --check`, `clippy -D warnings`, and `cargo test`. This is the **only place the Windows (WebView2) and Linux (WebKitGTK) cfg-gated code is fully compiled**: a macOS host cannot cross-compile the `ring` C build, so platform code is otherwise isolate-verified against `webview2-com`/`windows` in throwaway crates. The first Windows/Linux run is the real validation gate for those ports (and may surface platform-specific lints to fix).
- **Release** (`.github/workflows/release.yml`): on a `v*` tag (or manual dispatch), a serialized matrix builds each OS installer via `tauri-action` and attaches them to one draft release: a universal `.dmg` (macOS), `.msi` / NSIS `.exe` (Windows), `.deb` / `.AppImage` (Linux). `bundle.targets: "all"` + `createUpdaterArtifacts` produce the signed updater artifacts, and `includeUpdaterJson` merges the per-platform entries into one `latest.json` (the matrix runs `max-parallel: 1` so that merge never races). The Linux leg additionally **strips the bundled `libwayland-*` libs from the `.AppImage` and re-signs it**: the Ubuntu 22.04 build host ships libwayland 1.20, which AppImage puts ahead of the host copy on its library path, so on newer-Mesa systems (Fedora 43 / Arch / Bazzite) EGL fails with `EGL_BAD_PARAMETER`; removing the three libs makes the AppImage fall back to the host's (every GTK system has them). A final `patch-appimage-updater` job then rewrites the now-stale Linux signature in the merged `latest.json` to match the re-signed AppImage. The Tauri updater signing key is wired; **Apple code-signing / notarization is deferred** (an unsigned `.dmg` still builds), so the updater stays effectively dormant until signing is configured.

## Further reading

These `docs/architecture/` files elaborate on the sections above. They are not auto-loaded as agent memory; consult them when working in a subsystem. **If anything in a doc conflicts with WIS.md, WIS.md wins.**

- `docs/architecture/data-model-and-state.md`: `wis-core` immutable model, `wis-storage` schema/migrations, the IPC `mutate` pattern, coalesced/resilient persistence, and encrypted profile backup.
- `docs/architecture/privacy-and-network-engine.md`: `wis-network` (blocker, content rules, DoH, HTTPS-First, proxy, Tor), cosmetic filtering, scriptlets/user scripts, filter lists, and the fraudulent-site warning.
- `docs/architecture/security-vault-and-permissions.md`: `wis-secrets` crypto, vault + biometric gate, session-cookie encryption, camera/mic consent, capability isolation, CSP, navguard, and the wry patch.
- `docs/architecture/webviews-windows-and-overlays.md`: tab/chrome compositing, overlay webviews, the one-window-per-workspace model, engine-popup routing, and window styling/glass.
- `docs/architecture/cross-platform-and-page-channel.md`: the page->chrome channel per platform, keyboard dispatch, WebView2/GTK gotchas, native UX ports, the availability toast, and platform status.
- `docs/architecture/chrome-shell-and-layouts.md`: the React frontend structure, sidebar vs top-tabs layouts, workspaces/panes/pins/folders, theming, backgrounds, and the motion system.
- `docs/architecture/tabs-panels-and-media.md`: tab memory/sessions, quick features, Peek, reader, video popout, tab audio, page capture, downloads, widgets, bookmarks, history, and browser identity.
- `docs/contributing/linux-vm-testing.md`: contributor runbook for running and runtime-verifying the Linux (WebKitGTK) port in a macOS-hosted UTM/QEMU VM, with a per-feature verification checklist.

## Maintenance

When a change touches architecture, adds/removes a crate or module, or ships a user-facing feature, update the relevant section above in the same change. Treat WIS.md as the source of truth other contributors (and agents) load first.
