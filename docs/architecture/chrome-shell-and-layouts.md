# Chrome Shell & Layouts

This guide elaborates on WIS.md. If anything here conflicts, WIS.md wins.

## Frontend structure

The React/TypeScript chrome lives in `crates/shell/src/`. Imports use `@/...` aliases; relative cross-module imports are forbidden. The bundle is **window-agnostic**: it reads its own label from `getCurrentWindow()` and routes tab/overlay/peek/widget work to that window, so a secondary window behaves exactly like `main` over its own workspace.

- **Components** (`src/components/`): `Window` (root layout host), chrome frame (`TopBar`/`NavRow`/`TopTabsRail`/`Sidebar`/`WorkspacePill`), tab UI (`TabList`/`HorizontalTabs`/`TabFolderHeader`/`PinnedTabs`/`TabFavicon`), webview mounts (`TabWebViewHost`/`SplitPaneHost`, which measure rects and mount one native webview per visible pane leaf), and pages (`SettingsPage`, `HistoryPage`, `NewTabPage`, `DownloadsPage`).
- **Hooks** (`src/hooks/`): `useBrowserState` + `useBrowserStateEvents` (fetch + invalidate the `BROWSER_STATE_KEY` query, `staleTime: Infinity`), `useTabWebviews` (webview lifecycle), `useThemeApply` (state to DOM), `useFolderTarget` (folder selection), `useWorkspaceBackground`, `useKeyActions`, plus one hook per overlay surface.
- **Lib** (`src/lib/`): pure helpers, notably `theme.ts` (`themeVars`), `tabMotion.ts`, `paneActive.ts`, `tabStripLayout.ts`, `design.ts`, `glass.ts`.
- **Bindings** (`src/bindings/`): typed IPC wrappers (`commands.ts`) and types mirroring `wis-core` (`types.ts`). Wrappers accept camelCase and translate to serde's wire format at the boundary.

State is immutable end to end: `wis-core` mutations are pure `with_*` builders returning a new value, and the frontend re-renders off refetched query snapshots rather than local mutation.

## Render performance

Refetch and re-render are coalesced so unrelated state churn stays cheap:

- Browser-state-changed refetches are coalesced per animation frame: `useBrowserStateEvents` guards a single `requestAnimationFrame` before invalidating `BROWSER_STATE_KEY`, so a burst of native events triggers one refetch.
- React-Query structural sharing returns the same reference on a no-op refetch, so a refetch that changes nothing re-renders nothing.
- `useBrowserStateSelector` (a React-Query `select`) skips re-renders on an unchanged slice; `useSettings` and `useActivePrivacyMode` subscribe to just their slice rather than the whole state.
- Always-mounted leaves are `React.memo`'d (e.g. `Brand`), so they hold steady through unrelated parent re-renders.
- The startup widget-favicon prefetch runs through a concurrency cap of 4 (`WidgetFavicon.tsx` `PREFETCH_CONCURRENCY`), deferred a few seconds after mount, so a cold-cache launch does not flood the favicon path.
- `open_widget_panel`'s main-thread surface hop is awaited on the blocking pool, so the calling task never blocks the async runtime on it.

## Layout modes

`wis_core::layout::LayoutMode` has three variants: `SidebarLeft`, `SidebarRight`, `TopTabs`. `RailSide` (Left/Right) is the docked-controls edge; in `TopTabs` it decides which side the rail and its pinned circles anchor to, in sidebar layouts it mirrors the sidebar side. `Window` (the root host) picks the tree per mode.

- **Collapsible sidebar** (`Sidebar`) ends in a docked footer cluster (downloads, bookmarks, VPN, history, settings). It floats over the content area and is left transparent so it reveals the frame shadow rather than stacking a second dark layer. It auto-expands on hover, and a collapsed sidebar also expands when the cursor reaches the window's outermost edge column (document-level mousemove, since the window-resize handles sit on top of the in-aside hot strip and eat the first pixels).
- **Top-tabs** (`TopTabsRail` + `HorizontalTabs`) puts tabs in a horizontal strip with a docked rail of controls. The strip is corner-anchored opposite the rail: loose tabs and folder islands flow in one scrollable run (`.h-strip-run`, the strip itself never scrolls) while pinned circles stay anchored at all costs.
- **Top-tabs accordion limit**: `UserSettings.top_tabs_max_expanded_folders` (default 2, 0 = no limit; a segmented 1/2/3/No-limit control in Settings -> Layout, in the settings blob, no migration) caps how many folder islands stay expanded at once. Expanding one island past the limit collapses the least-recently-expanded island (session-scoped per-workspace recency, seeded in position order) and **sleeps** its members. `MEMBER_TAB_MAX` (150, or 120 compact, `lib/tabStripLayout.ts`) caps an expanded island's member-tab width, enforced only in the top-tabs UI (the sidebar tree is unrestricted).

## Workspaces, panes, pins, folders

- **Workspaces**: color-accented profiles, each with its own WebView data store, proxy, and privacy mode. One OS window per workspace is a hard invariant (`set_active_workspace` and `new_window` focus an existing owner window rather than duplicate). Store isolation is the WKWebView `data_store_identifier` on macOS and a per-workspace WebView2 user-data folder on Windows (`commands::workspace_webview_data_dir`).
- **Private browsing**: a private workspace is *ephemeral* (non-persistent data store), excluded from save and wiped on close. Its `selected_folder_id` and session state never touch disk.
- **Split panes**: a per-workspace binary `PaneNode` tree (`wis_core::pane`), `Leaf { tab }` or `Split { dir: SplitDir, first, second }`. Each leaf mounts its own native webview to a measured rect. `SplitPaneHost` handles resizable dividers and focused-leaf highlight; native click-to-focus (`pane_focus.rs`) routes keybinds to the focused leaf. Panes are in-window multitasking; new windows are for cross-workspace multitasking.
- **Pinned tabs**: `Workspace.pinned_tab_ids` + `Tab.pinned`, kept in sync by `pin_tab`. Pinned tabs leave the normal list and render as a docked quick-access set in every layout (favicon grid in the sidebar, favicon circles at the rail-nearest end of the top strip). They never sit inside a folder (they sleep like any background tab). `reorderedWithPinned` reorders only the non-pinned subset and merges back so pinned slots survive in `tab_order`.
- **Tab folders**: `wis_core::tab_folder::TabFolder { id, name, accent, collapsed, position, icon }` group *live* tabs (distinct from bookmarks, which save URLs). `Workspace.tab_folders` + `Tab.folder_id`, mutually exclusive with pinned. Selecting a folder (theme-ring highlight) targets it for new tabs; the shared `useFolderTarget` hook (`setTarget`/`syncToTab`) keeps sidebar and top-tabs identical, tracks the focused tab, and clears on a loose/pinned tab. `+ New tab` always makes a loose top-level tab (`with_new_loose_tab`). Collapsing a folder **sleeps** its members immediately, bypassing the idle timer. `selected_folder_id` is session-only, reset to `None` on load; folders themselves persist in the JSON `tab_folders` column.
- **Workspace accents**: `UserSettings.custom_accent_palette` (shared across workspaces, in the settings blob) renders as the workspace popover accent grid's saved swatches, plus a dashed `+` square that spawns a tentative swatch via the `ColorPicker`. `Workspace.custom_accent` (a validated hex value, migration `M0016`) paints the workspace dot, and is cleared when a preset accent is picked (`with_custom_accent`).

## Icons

Folders and workspaces carry an icon rendered inside their color dot (`set_tab_folder_icon` / `set_workspace_icon`). Encoded as a compact string (`glyph:<name>` | `glyph-fill:<name>` | `emoji:<char>`), validated by pure `wis_core::icon::parse_icon_ref` at the command boundary; glyph names are charset-checked in core and resolved in the chrome (`src/lib/iconGlyphs.tsx`, Lucide-derived wire+fill shapes), unknown names fall back to the plain dot. No migration: `TabFolder.icon` rides the JSON `tab_folders` column and `Workspace.icon` repurposes an existing unused column. Glyphs tint with `currentColor`; emoji render as-is. Picked via the shared `IconPicker` (Line/Fill/Emoji tabs + None reset).

## URL-bar suggestions

An overlay dropdown (`useUrlbarSuggest` + `lib/urlbarSuggest.ts`): recent searches, history-ranked site recommendations, inline base-website completion, favicons. Because native tab webviews composite above the chrome, this is a separate overlay webview positioned natively, not HTML in the chrome. Default search engine (DuckDuckGo/Google/Bing) is a setting. On Windows the close path supersedes any in-flight refresh (bumps a request id) so a late `showUnderBar` can never re-show the just-closed overlay over the content column. Click-away closes through two fallbacks beyond the input's own blur (which never fires when OS focus jumps to a tab webview): a window `blur` listener funnels into the same delayed close (overlay row clicks cancel it via the pick event), and a capture-phase outside `pointerdown` in the chrome closes immediately. The same hook also powers the **launchpad omnisearch** (`NewTabPage`, anchored to its form so the dropdown takes that width): same suggestion rows, engine resolution, and search recording as the top bar; the dropdown opens on click / Cmd+L / typing, not on the mount autofocus. Open-tab resolution + switching is shared via `useOpenTabActions`.

## Launchpad + homepage

The new-tab page (`NewTabPage`) is configurable via `wis_core::new_tab`: `NewTabTarget`, `ShortcutTile`, with boundary sanitizers `sanitize_homepage` / `sanitize_shortcuts`. Quick-access tiles and history rows honor the chrome middle-click contract (`onAuxClick` -> `create_background_tab`, opening a background tab and staying on the page). The launchpad also hosts the per-workspace background: it reads `--wp-grad` and the wallpaper layers straight off `<body>`, so a background themes the page with zero component changes.

## Backgrounds

Per-workspace, `Workspace.background: Option<Background>` (`wis_core::background`), persisted as JSON; ephemeral workspaces keep it in memory only. Each mode's last configuration persists in `Workspace.background_drafts` so switching modes restores what the user built in each.

- **Gradient**: `GradientSpec` (angle + 2..8 hex stops, validated at the IPC boundary), edited live from `BackgroundEditor` (angle capped 0..=180 since larger angles mirror smaller ones, per-stop `ColorPicker` in `controls.tsx`). Rendering overrides the `--wp-grad` token on `<body>` via `useWorkspaceBackground`.
- **Image/GIF**: `import_wallpaper` validates by magic bytes only (png/jpeg/gif/webp), caps at 20 MB, stores content-addressed at `<app_data>/wallpapers/<sha256>.<ext>`. Served through `wis-asset://wallpaper/<hash>` (answers only `main`/`overlay-*` labels, hash strictly 64 hex chars so requests cannot escape the store). `Background::Image { asset, fit, dim, blur }`: fit maps to cover/contain/tile, blur 0..=40 px blurs the wallpaper without content, dim 0..=80% layers a black overlay. The launchpad paints it as one shared canvas spanning the whole content host, so split panes reveal contiguous slices of a single wallpaper with blur seams aligned.

## Theming

`useThemeApply` is the single sink that mirrors settings onto the DOM (verified in `hooks/useThemeApply.ts`):

- Theme mode: `data-mode` on `<body>` (Light/Dark/System). Also pushed to web content (`apply_tab_appearance`): macOS `NSAppearance` DarkAqua/Aqua, Windows WebView2 `PreferredColorScheme` (per-profile), so a page's `prefers-color-scheme` matches the in-app theme.
- Accent: `data-accent`; a custom theme's inline `--primary-50..900` vars beat the `[data-accent]`/`[data-mode]` rules.
- Design (`wis_core::design::ChromeDesign`): `data-corners` and `data-glass` drive `--radius-scale` / `--glass-scale` multipliers in `tokens.css`; `data-tab-shape`, `data-folder-style`, `data-tab-indicator`, `data-density`, `data-close-btn` select `window.css` variant blocks; `--sb-w` is set inline. Glass is scoped: `glass-on` is the master toggle, gated per surface by `glass-chrome`/`glass-menus`/`glass-tabs`.
- **Custom themes** (`ThemesSection`): `CustomTheme` carries light+dark `ModeTokens` (accent + bg/surface/raised/chrome/content/text/secondary/border, all validated hex via `wis_core::theme`); the accent expands into the whole scale at apply time (`src/lib/theme.ts::themeVars`, pure mixes toward white/black). `set_active_theme` injects the vars inline, so the chrome re-themes and reverts to presets with zero component changes. Sharing moves versioned `.wistheme` JSON (import size-caps, validates the full structure, rejects newer versions, re-mints the id so a shared theme cannot collide) or portable `wis-theme:<base64>` clipboard codes.

## Motion

Central motion tokens in `tokens.css` (`--dur-fast/med/slow`, `--ease-standard`, springy `--ease-emphasized`, `--ease-exit`) back every chrome transition; the scattered hardcoded durations were folded into them. `UserSettings.motion` (`MotionLevel`: Full/Reduced/Off) maps to body classes in `useThemeApply`: **Reduced** keeps durations but flattens `--ease-emphasized` to standard (no overshoot), **Off** zeroes transition/animation durations, and OS `prefers-reduced-motion` caps Full at Reduced live.

- **Drag-reorder** (`lib/tabMotion.ts`): the source hides under a floating ghost while siblings **live-part** one slot around the insertion point (`body.is-tab-dragging`). Hit-testing measures **parting-free** rects (`layoutRect` = live rect minus transform), else the drop boundary chases the pointer. Reorders and folder assignment apply optimistically (`patchTabOrder`/`patchFolderOrder`), then the drop glides from the ghost into its slot with a settle pop (WAAPI, dropped on Reduced/Off).
- **Closing a tab** is a FLIP glide, not a collapsing slot. The closing tab leaves the sizing inputs and goes `display: none` on the close frame (slot and gap gone at once); survivors snapshot their rects (`captureFlip`) and animate from old to new with `translateX` + `scaleX` (position and width in lockstep). Closing the active tab predicts the promotion locally (`lib/paneActive.ts`, a pure mirror of `with_tab_removed` then `with_pane_after_removed`) so it sizes right before the refetch. Triggered synchronously over a same-tick DOM event, deduped per id; a new close cancels the in-flight FLIP.
- **Native surfaces** (panes, widget panels, Peek) animate by stepping webview bounds per frame over IPC; a live page is never resized (reflow-per-frame reads as jank). Split entrances glide a leaf's flex-grow from zero while the other webviews tuck away (`.is-entering`); removals keep painting the outgoing tree and glide the closing subtree shut (`.is-exiting`) before the committed tree reveals.
- Active-tab markers are single sliding bars measured against the list root (`.kb-active-bar`, `.kb-settings-nav-ind`), not per-row pseudo-elements. Workspace switches slide the tab list/strip in from the new workspace's relative side (`workspaceSlideIn`, imperative WAAPI, no remount).
