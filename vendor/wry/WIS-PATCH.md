# Vendored wry 0.55.1 (patched)

Verbatim copy of wry 0.55.1 from crates.io, applied via `[patch.crates-io]`
in the workspace `Cargo.toml`, with a single fix:

- `src/wkwebview/mod.rs`, `url_from_webview`: `WKWebView.URL` can be nil in
  edge states (observed: clicking "go back" on WebKit's fraudulent-website
  interstitial when the back stack is empty, because the warned navigation
  never committed). The upstream `unwrap()` panicked inside the navigation
  delegate (an objc callback), which aborts the whole process ("failed to
  initiate panic"). The patch returns `Err(Error::Io(..))` for the nil
  cases; the navigation delegate's `unwrap_or_default()` decays that to "".

The fix is applied **verbatim from the open upstream PR**
<https://github.com/tauri-apps/wry/pull/1729> ("fix(macos): avoid panicking
on missing webview URL"), so when that PR merges and ships, dropping this
vendor + the `[patch.crates-io]` entry is a zero-behavior-change swap.
Related: <https://github.com/tauri-apps/wry/pull/1744> fixes the same nil
unwrap pattern in `url_scheme_handler` (a different crash site, also open).

Removal checklist: a wry release containing #1729 exists -> delete
`vendor/wry` and the `[patch.crates-io]` entry, `cargo update -p wry`, and
re-test: fraud-warning page -> "go back" on a tab whose warned URL was its
first navigation.

## Second patch: webview2 container leak on failed creation

- `src/webview2/mod.rs`, `new_in_hwnd`: the container HWND is created before
  the WebView2 environment/controller, and any failure in those steps
  early-returned without `DestroyWindow`. The leaked container is
  `WS_VISIBLE`, paints nothing (null class brush) and is fully hit-testable,
  so it silently swallowed mouse input over its rect for the process
  lifetime (observed: a failed Peek webview creation left the page center
  wheel-dead and click-dead). A drop guard now destroys the container on
  any failed creation step, defused once `InnerWebView` takes ownership
  (its `Drop` destroys child hwnds, so a later `set_bounds` failure on a
  child webview is covered). Scope: the guard is defused before the final
  `set_bounds`/`resize_to_parent`; a `resize_to_parent` failure on a
  **non-child** webview still relies on the parent window's teardown to
  reap the container (unchanged from upstream), which Wis never exercises
  (every Wis webview is `add_child`, i.e. a child). Not yet upstreamed;
  re-apply (or upstream) when bumping wry.

## Third patch: Linux positioned child webviews honor set_bounds

Part of the three-crate Linux multi-webview fix; see also `vendor/tao`
(`content_fixed` overlay) and `vendor/tauri-runtime-wry` (routes child
webviews into it). Upstream: tauri-apps/wry#1745, tauri-apps/tauri#10420.

- `src/webkitgtk/mod.rs`, `set_bounds`: when a webview lives in a `gtk::Fixed`
  (`is_in_fixed_parent`), upstream repositioned it with a bare
  `webview.size_allocate(x, y, w, h)`. `gtk::Fixed` re-runs `size_allocate` on
  every layout pass and resets each child to its stored `put()` coordinate, so
  the requested bounds were dropped on the first relayout (window resize, tab
  add/remove, any `queue_resize`); positioned child webviews snapped back and
  overlapped at (0,0). The patch first calls `fixed.move_(&webview, x, y)` and
  `webview.set_size_request(w, h)` (both persist across layout passes),
  recovering the parent `gtk::Fixed` via `webview.parent().downcast()` (no
  stored handle needed), then keeps the immediate `size_allocate`. Backend-
  agnostic pure GTK (no X11 API), works on X11 and Wayland. Applied verbatim
  from wry#1745; drop when it merges and ships.

## Fourth patch: Linux bounds() reports the child's real position (Wis-original)

Follow-up the third patch's upstream (wry#1745) explicitly deferred: its notes
say *"on this path `WebView::bounds()` doesn't report the child's position
(returns (0,0))… Happy to follow up separately."* No upstream PR fixes it yet,
so this one is **Wis-original**.

- `src/webkitgtk/mod.rs`, `bounds()`: on the non-X11 (`gtk::Fixed`) path,
  upstream read only the widget's `allocated_size()` and left the returned
  position at `(0,0)`. `tauri-runtime-wry` rebuilds a `Rect` from `bounds()` in
  its `SetSize` (and `SetAutoResize`) handlers, overwriting only the size,
  then calls `set_bounds`, so every child webview was re-planted at the window
  top-left whenever it was resized (and drifted there again on window resize via
  the poisoned auto-resize rates). With the third patch making `set_bounds`
  actually stick, this reset became visible.
- The patch reads position and size from the durable sources `set_bounds`
  writes rather than the live allocation: position from the `gtk::Fixed` child
  properties (`fixed.child_property::<i32>("x"/"y")`, valid whether or not the
  widget has ever been allocated) and size from `size_request()` (falling back
  to the allocation when unset). The live allocation alone is not enough: GTK
  skips `size_allocate` for hidden widgets, so a webview positioned while
  hidden (overlay popups are positioned before being shown) read back a stale
  `(-1,-1)` allocation and the SetSize round-trip re-planted it at the window
  top-left on show (observed: the URL-suggestions dropdown rendering at the
  browser's top-left corner). The `use dpi::LogicalPosition` import is made
  unconditional because it is now used in this always-compiled fallback, not
  just the `#[cfg(feature = "x11")]` branch.

Removal: fold into the wry#1745 drop — once a wry release reports the child's
position on this path (this follow-up or an equivalent), delete along with the
third patch.

## Fifth patch: Linux map finishes a fixed-parent webview's show (Wis-original)

Two first-show defects for a webview in a shared `gtk::Fixed`, fixed together
in a `connect_map` handler installed in `add_to_container` (plus a small
`set_visible` companion), because both are "show must finish the job"
semantics:

- Stale geometry: a webview positioned while hidden keeps a stale allocation
  (GTK skips `size_allocate` for non-visible widgets, and mapping does not
  re-allocate), so on first show it stayed invisible until an external
  relayout, e.g. a window resize, re-ran the fixed's layout (observed: the
  find bar and the Ctrl+L overlay appearing only after resizing the window).
- Back-of-stack: siblings in a `gtk::Fixed` stack by add order and GTK has no
  implicit raise-on-show; Wis creates its overlay webviews once at startup and
  lazily creates tab webviews later, so an opened overlay displayed behind the
  tab webview (observed: Ctrl+F opening an invisible find bar).

Doing this work at `show()` time is not enough; GTK defers map/realize to the
next layout pass, so at show the widget can be unrealized (no GdkWindow: the
allocation does not reach the window and `window()` is `None` for the raise),
which is why a first `set_visible`-based version still needed a window resize.
The map signal fires exactly when the GdkWindow exists, so the handler
re-asserts the durable geometry there (`gtk::Fixed` child position + size
request, the fourth patch's sources; skipped when bounds were never applied),
raises the GdkWindow above its siblings, and queues a draw. It runs on every
map, so each re-show of a hidden overlay is finished the same way.
`set_visible(true)` additionally raises an already-mapped webview (no map
signal fires, but show still means "bring to front", matching macOS/Windows
overlay semantics); tab switching is unaffected in practice because the app
only calls show on a visibility transition.

Scoped to fixed-parented webviews; the X11-reparent and box-packed paths are
untouched. Not yet upstreamed; belongs with the wry#1745 family.

## Patch: focused child webviews get Cmd-key events (macOS)

- `src/wkwebview/class/wry_web_view.rs`, `performKeyEquivalent:`: upstream's
  workaround for tauri#9426 returns `NO` for every child webview, so the
  main-menu accelerator always wins and the page never sees Cmd-key events in
  JavaScript. In a multi-webview browser that breaks web apps with their own
  shortcut handling: Google Docs / Slides Cmd+Z, Cmd+A, Cmd+Shift+V do
  nothing, because the menu's `undo:` / `selectAll:` selectors drive native
  editing commands a canvas editor ignores. The patch refines the guard: a
  child that is (or contains) the window's first responder calls `super`, so
  WKWebView forwards the event to its page first (Safari behavior) and
  WebKit's own re-dispatch delivers page-declined events onward to the menu;
  unfocused children keep returning `NO`, preserving the #9426 fix (only one
  webview may claim an event, the menu never starves).

This is a Wis-authored refinement, not an upstream PR drop. If wry lands a
proper fix for tauri#9426 (focused-webview key routing), re-evaluate against
it. Removal checklist: with the patch dropped, in Google Docs on a tab
webview press Cmd+Z / Cmd+A / Cmd+Shift+V (must reach the page) and Cmd+T /
Cmd+W on an ordinary page (menu must still fire).

Reserved-combo carve-out (required in practice): runtime testing showed
WebKit's page-declined resend does NOT reach the menu in this embedding, so
forwarding menu-bound combos to the page killed Cmd+W / Cmd+T / Cmd+L
whenever a page held focus. `menu_reserves_event` therefore walks the live
main menu per key event: a combo bound to a menu item is menu-first
(returns NO so the accelerator fires), EXCEPT items whose action is
`undo:` / `redo:` / `selectAll:`, which stay page-first. Walking the live
menu keeps the reserved set correct under Wis's rebindable shortcuts.

Why exactly those three: their selectors produce no DOM event a canvas-style
web app can observe, so Google Docs-class apps need the raw keydown in JS.
Clipboard selectors (`cut:` / `copy:` / `paste:`) DO fire DOM clipboard
events such apps handle, so those combos stay menu-first; making them
page-first broke Cmd+C/V on ordinary pages (no resend = no native
fallback). The three page-first combos get their native fallback from the
embedder instead: Wis's `edit_keys` module injects a watcher that posts
unconsumed combos to `wisEditKey`, and the handler replays the selector via
`sendAction` down the responder chain.

Known edge: Cmd+Shift+digit combos are not matched by the walk
(`charactersIgnoringModifiers` applies Shift to digits) and so fall
page-first; none of the default bindings use that shape.
