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
