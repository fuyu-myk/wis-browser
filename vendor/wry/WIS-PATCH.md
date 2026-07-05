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
