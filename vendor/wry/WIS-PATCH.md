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
  actually stick, this reset became visible. The patch reads position **and**
  size from `self.webview.allocation()` (the widget's post-layout allocation
  carries the `gtk::Fixed::move_` position). The `use dpi::LogicalPosition`
  import is made unconditional because it is now used in this always-compiled
  fallback, not just the `#[cfg(feature = "x11")]` branch.
- Caveat: `allocation()` x/y are only meaningful after the first size-allocate
  (pre-realize they read `(-1,-1)`), which is fine for runtime `SetSize`/resize
  (post-layout); exactly where the reset bit. One latent GTK-visibility edge
  (resizing while a background/hidden tab exists, since GTK skips `size_allocate`
  for non-visible widgets) is worth a runtime check but is not a regression
  (upstream forced `(0,0)` for the visible case too).

Removal: fold into the wry#1745 drop — once a wry release reports the child's
position on this path (this follow-up or an equivalent), delete along with the
third patch.
