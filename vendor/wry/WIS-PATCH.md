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

## Fourth patch: expose download expected size (macOS)

- `src/wkwebview/download.rs`, `download_policy`: the delegate receives the
  `NSURLResponse` whose `expectedContentLength` is the download's total byte
  count, and upstream discards it (the `DownloadEvent::Requested` signature
  carries only url + destination, and changing it would mean vendoring
  `tauri-runtime` + `tauri` too). The patch stashes the value in a
  thread_local (`src/wkwebview/mod.rs`) immediately before invoking the
  started handler; `wry::last_download_expected_bytes()` reads it back.
  Sound because tauri-runtime-wry invokes the tauri `Requested` handler
  synchronously in the same main-thread call stack as `download_policy`
  (verify on every wry/tauri-runtime-wry bump: `downloads.rs::on_requested`
  must run inside the started callback, not deferred). -1 means the server
  sent no Content-Length (chunked); the shell maps that to an indeterminate
  spinner. Not upstreamable as-is (upstream would extend the event type);
  re-apply when bumping wry.

Removal checklist: wry ships download progress or a richer Requested event
-> drop the thread_local + `download.rs` line + the `lib.rs` re-export, move
`downloads.rs::on_requested` onto the upstream API, and delete the direct
`wry` dependency from `crates/shell/src-tauri/Cargo.toml` if nothing else
uses it.

## Fifth patch: cancel in-flight downloads (macOS)

- `src/wkwebview/download.rs`: WKDownload exposes `cancel:` but wry never
  hands the download object out, so an embedder cannot stop a download.
  The patch keeps a main-thread registry of live downloads keyed by their
  destination path (registered when `download_policy` accepts, removed in
  `download_did_finish`/`download_did_fail`), and exposes
  `wry::cancel_download_to(path) -> bool`. An explicit cancel does NOT
  reach the fail delegate (WebKit's `DownloadManager::cancelDownload`
  passes `IgnoreDidFailCallback::Yes`, suppressing the DidFail IPC), so
  `cancel_download_to` unregisters the entry itself, before making the
  ObjC call (also keeps the registry borrow from spanning reentry).
  Bounded leak note: a download whose finish/fail callback never fires
  (e.g. delegate torn down mid-flight at app exit) leaves its registry
  entry until process end; entries are two pointers + a path.

Removal checklist: wry ships a download handle/cancel API -> drop the
registry + `cancel_download_to` + both re-exports, and move
`downloads.rs::cancel_download` onto the upstream API.

## Sixth patch: surface provisional navigation failures (macOS) (Wis-original)

- The `WKNavigationDelegate` (`src/wkwebview/class/wry_navigation_delegate.rs`)
  implements no `didFailProvisionalNavigation:withError:`, so a DNS failure,
  refused connection, timeout, or TLS error produces no page-load event: the
  navigation never commits (no `Started`) and never finishes, leaving the
  tab's loading state stuck until the shell's progress poller times out
  (~30s). `PageLoadEvent` has only `Started`/`Finished` and cannot carry a
  `Failed` variant without also vendoring `tauri-runtime`/`tauri` (the same
  constraint as the download expected-size patch), so the failure rides the
  existing `Finished` event plus a thread_local side channel.
- `src/wkwebview/navigation.rs` (`did_fail_provisional_navigation`): stashes
  the `NSError` code + domain, the failing leaf certificate, and the failing
  URL (`NSURLErrorFailingURLErrorKey`; the webview's own URL is still the page
  navigated away from) via `set_last_page_load_error`, emits
  `PageLoadEvent::Finished`, then clears the channel with
  `take_last_page_load_error()`. `src/wkwebview/mod.rs`: the side channel with
  `page_load_error_pending()` and `take_last_page_load_error()`. The channel
  is shared by every WKWebView on the main thread (tabs, peek, widget panels,
  overlays), but only a tab's page-load handler consumes it, and only during
  the synchronous emit; the unconditional post-emit clear is what guarantees a
  failure on a non-consuming webview cannot leak into the next tab's load. The
  shell reads `wry::take_last_page_load_error()` in its `on_page_load` closure
  and the failing URL from tab state (the target URL, written before
  navigation).
- `src/wkwebview/class/wry_navigation_delegate.rs`: the `custom_handler` that
  wraps the page-load handler substitutes a parseable `about:blank` when
  `WKWebView.URL` is nil AND a failure is pending, because a provisional
  failure leaves the URL nil and `tauri-runtime-wry` drops any `Finished`
  whose URL fails to parse. The substitution is gated on a pending failure so
  successful loads are untouched.
- Sound because tauri-runtime-wry invokes the tauri page-load handler
  synchronously in the same main-thread call stack as the delegate callback
  (verify on every wry/tauri-runtime-wry bump: the shell's `on_page_load`
  closure must run inside the delegate callback, not deferred, so the side
  channel is read before it is overwritten by the next navigation).
  Provisional failures only; `didFailNavigation:` (post-commit) is left
  unhandled so a partially-rendered page is not replaced. NSURLErrorCancelled
  and other user-cancel codes are filtered shell-side (`wis_core::LoadError`),
  not here.

Precedent that upstream does not move on this: wry#1714 (add
`didStartProvisionalNavigation` to `PageLoadEvent`) shipped a working patch
and was closed unmerged after the reporter patched locally.

Removal checklist: wry ships a load-failure page-load event -> drop the
thread_local + `did_fail_provisional_navigation` + the delegate method + the
`custom_handler` URL fallback + the `lib.rs` re-export, and move the shell's
`load_error` macOS arm onto the upstream API.

## Seventh patch: server-trust challenge evaluator (macOS) (Wis-original)

DORMANT. Wis sets a Network.framework proxy config on every tab webview's
data store (the loopback privacy proxy). That layer resolves TLS trust
internally, so `webView:didReceiveAuthenticationChallenge:completionHandler:`
is never invoked while the proxy is active (verified: with the proxy removed
the delegate fires for both server-trust and HTTP-basic; with it on, neither
does). The evaluator below is therefore never exercised, and no shell code
registers it. The patch is retained pending an architecture decision on
whether trust evaluation moves into the proxy/nw layer; drop it if that
lands elsewhere. See `memory/auth-challenge-delegate-not-firing.md`.

WebKit rejects an untrusted TLS certificate during the provisional load
unless `webView:didReceiveAuthenticationChallenge:completionHandler:` answers
the server-trust challenge with a credential. `WKNavigationDelegate`
implements no such method upstream, so an embedder cannot offer a "proceed
anyway" bypass on its TLS interstitial.

- `src/wkwebview/mod.rs`: a process-wide evaluator registered once via
  `wry::set_server_trust_evaluator(Box<dyn Fn(&str, &[u8]) -> bool + Send +
  Sync>)` (`OnceLock`, first registration wins), called with the challenge's
  `(host, leaf certificate DER)` so the embedder pins the exact certificate,
  not the whole host. A static rather than a builder attribute because tauri
  constructs the `WebViewBuilder` (same constraint as the download-size
  patch).
- `wry_navigation_delegate.rs` + `navigation.rs`: the delegate answers a
  challenge whose method is `NSURLAuthenticationMethodServerTrust` and whose
  `(host, leaf DER)` the evaluator approves with `UseCredential` +
  `credentialForTrust:`. Every other path (no evaluator, evaluator declined,
  unreadable chain, non-server-trust method such as HTTP basic auth or client
  certificates, nil trust) calls `PerformDefaultHandling`, which is identical
  to not implementing the method at all. `SecTrustRef` has no
  objc2-foundation binding, so `serverTrust`/`credentialForTrust:` use raw
  `msg_send!` with an opaque pointer. The leaf DER is read via raw
  Security/CoreFoundation externs: `SecTrustGetCertificateAtIndex(_, 0)`
  returns a borrowed ref (no release) and per SecTrust.h does not trigger a
  trust evaluation; `SecCertificateCopyData` follows the Copy rule and is
  released after the bytes are copied out.
- `did_fail_provisional_navigation` also reads the failing server's leaf
  certificate from the `NSError`'s `NSURLErrorFailingURLPeerTrustErrorKey`
  userInfo and bundles its DER into the page-load-error side channel
  (`(code, domain, Option<DER>)`). This binds the pinnable certificate to the
  navigation that failed, so a concurrent connection to the same host cannot
  substitute a different certificate into what the embedder pins (the shell
  freezes it per tab). `None` for non-TLS failures.
- `Cargo.toml`: objc2-foundation gains `NSURLAuthenticationChallenge`,
  `NSURLCredential`, `NSURLError`, `NSURLProtectionSpace`, `NSURLSession`
  (types and constants only).

Soundness: the completion handler is called exactly once on every path. The
evaluator is process-wide, so an approved (host, certificate) pair is
accepted in every WKWebView for the session, matching the shell's
`tls_bypass` scope (the only registrar). A different certificate for the same
host falls back to default handling. The block signature matches Apple's
`(NSURLSessionAuthChallengeDisposition, nullable NSURLCredential *)`.

Removal: when wry ships an authentication-challenge API, drop the static,
both delegate additions, the `Cargo.toml` features, and the `lib.rs`
re-export, and move the shell's evaluator registration onto it.

## Seventh patch: forced main-frame downloads (macOS) (Wis-original)

- `src/wkwebview/navigation.rs` (`navigation_policy_response`): upstream only
  routes a response to the download delegate when `canShowMIMEType` is false,
  so a PDF (and any attachment WebKit can render: images, text) opens in
  WebKit's inline PDF viewer, whose HUD buttons ("open in Preview", download)
  call private UI-delegate hooks wry does not implement, leaving the tab with
  no way back. The patch also asks the embedder policy for main-frame
  responses: `wry::set_main_frame_download_policy(mime_types, attachments)`
  (`src/wkwebview/mod.rs`, a process-wide `RwLock`) forces the Download
  policy for the listed MIME types and, when `attachments` is set, for any
  `Content-Disposition: attachment` response (`NSHTTPURLResponse
  valueForHTTPHeaderField:`, macOS 10.15+). The MIME types apply to the main
  frame only (`isForMainFrame`), so embedded PDF previews keep rendering
  inline; attachments download from any frame since a page that loads one
  in a hidden iframe otherwise gets nothing. Wis sets `["application/pdf"]` +
  attachments at setup.

Removal checklist: wry exposes a navigation-response policy hook -> move the
policy into the shell's handler and drop the static + `lib.rs` re-export.

## Eighth patch: hidden child webviews are parked off-window (macOS) (Wis-original)

- `src/wkwebview/mod.rs` (`set_visible`, `park`, `bounds`, `set_bounds`):
  AppKit resolves a drag destination from the frontmost view under the
  cursor without checking the hidden flag, then only walks up that view's
  ancestors. Wis stacks hidden overlay webviews (palette, window picker,
  navguard, ...) and hidden background tabs above the visible tab, so an
  in-page HTML5 drag was delivered to whichever hidden view sat on top: a
  registered one answered `NSDragOperationNone` from its own WebKit, an
  unregistered one yielded no destination at all, and the page underneath
  never received `dragover`/`drop` (GitHub pin reorder, any sortable list).
  Peek worked only because it is the newest child and sits above the
  overlays. A hidden child webview is now moved to (-100000, -100000) while
  it keeps its logical frame: `bounds` reports the parked frame and
  `set_bounds` updates it, so tauri's SetSize round-trip and Wis's
  show-then-position sequences see no change; showing moves the view back.
  Webviews created hidden go through the same path.

Removal checklist: wry hides child webviews in a way AppKit excludes from
drag hit-testing (or upstream adopts this) -> drop the field, `park` and
the `bounds`/`set_bounds` branches.

## Ninth patch: per-navigation webpage preferences hook (macOS) (Wis-original)

WebKit consults `webView:decidePolicyForNavigationAction:preferences:decisionHandler:`
in place of the plain variant when the delegate implements it, and the
`WKWebpagePreferences` it passes is the only place a page-level policy such as
autoplay (`_autoplayPolicy`, SPI) can be set after a webview exists; the
configuration knob is creation-time and tauri constructs the builder.

- `src/wkwebview/mod.rs`: a process-wide hook registered once via
  `wry::set_webpage_preferences_hook(Box<dyn Fn(&str, *mut c_void) + Send + Sync>)`
  (`OnceLock`, first registration wins), called with the target URL and the
  preferences object as a raw pointer so the embedder's objc2 version does not
  have to match wry's.
- `wry_navigation_delegate.rs` + `navigation.rs`: the delegate gains the
  preferences variant. On a main-frame navigation it runs the hook, then calls
  the existing `navigation_policy` with a wrapping block that returns the same
  preferences alongside the policy, so download and navigation-handler
  behaviour is unchanged. Subframe navigations skip the hook: WebKit applies
  website policies from the main frame only.

## Tenth patch: embedder decides who gets the `window.ipc` global (Wis-original)

Every backend defines `window.ipc` on every page as a frozen, non-writable,
non-configurable global for Tauri's IPC. A page with a global of its own by
that name then throws in strict mode as soon as it assigns it; such pages
stop before drawing anything, leaving its dark backdrop. Tauri refuses calls
from remote webviews anyway, so the global only does harm there.

- `src/lib.rs`: a process-wide hook registered once via
  `wry::set_ipc_global_hook(Box<dyn Fn(&str) -> bool + Send + Sync>)`
  (`OnceLock`, first registration wins), asked with the webview's id. Without
  a hook every webview gets the global, as upstream.
- `src/wkwebview/mod.rs`, `src/webkitgtk/mod.rs`, `src/webview2/mod.rs`: the
  `window.ipc` init script is added only when the hook allows it. The `ipc`
  message handler itself is untouched.

## Eleventh patch: `window.print()` opens the print sheet (macOS) (Wis-original)

WebKit hands `window.print()` to the UI delegate's private
`_webView:printFrame:pdfFirstPageSize:completionHandler:`, and upstream wry
does not implement it, so the call did nothing. Tauri covers this with its own
`window.print` over IPC, which only app webviews can use.

- `src/wkwebview/class/wry_web_view_ui_delegate.rs`: the delegate implements
  it. It calls the completion handler first (WebKit holds the page until then,
  and printing needs the page), then runs
  `_printOperationWithPrintInfo:forFrame:` for the calling frame as a sheet on
  the webview's window, so an iframe prints only itself. Nothing happens when
  the webview is hidden, has no window, or its window already shows a sheet.

## Twelfth patch: pages see the window's frame (macOS) (Wis-original)

WebKit asks the UI delegate's private
`_webView:getWindowFrameWithCompletionHandler:` for `window.outerWidth`,
`outerHeight`, `screenX` and `screenY`; upstream wry does not implement it,
so all four were 0.

- `src/wkwebview/class/wry_web_view_ui_delegate.rs`: the delegate answers
  with the webview's window frame (zero without a window). WebKit converts it
  to screen coordinates for the page.

## Thirteenth patch: the embedder can offer menu combos to the page first (macOS) (Wis-original)

The Cmd-key patch above keeps every combo bound to a non-editing menu item
menu-first, so a web app never sees Cmd+P, Cmd+F or Cmd+S. Browsers give the
page those first and keep only a few (new/close tab, address bar).

- `src/wkwebview/mod.rs`: `wry::set_page_first_menu_hook(Box<dyn Fn(&str,
  &str, bool) -> bool + Send + Sync>)` (`OnceLock`, first registration
  wins), asked with the focused child's webview id, the matched menu item's
  title and whether it is an editing item.
- `src/wkwebview/class/wry_web_view.rs`: `WryWebViewIvars` carries the
  webview id; `menu_claims` asks the hook for every matched item, so the
  embedder can also keep undo/redo/select all on the menu for webviews
  without a replay fallback (Wis's own interface, a child under Tauri's
  multiwebview mode, lost Cmd+A in its inputs). The
  page-declined resend still never reaches the menu, so the embedder runs
  the action itself when the page leaves the combo alone (Wis: the macOS
  `wisKeybind` forwarder).

## Fourteenth patch: the embedder names a webview's profile (Windows) (Wis-original)

WebView2 keeps per-profile state such as the preferred colour scheme, and
every webview of an environment shares its default profile. Tauri's builder
has no profile option, so Wis could not keep its canvas tabs' theme apart
from its web tabs' without a separate environment (another browser process).

- `src/webview2/mod.rs`: `wry::set_profile_name_hook(Box<dyn Fn(&str) ->
  Option<String> + Send + Sync>)` (`OnceLock`, first registration wins),
  asked with the webview id in `new`; `create_controller` passes the name to
  `ICoreWebView2ControllerOptions::SetProfileName`. A runtime without
  `ICoreWebView2Environment10` refuses a named profile (`E_NOINTERFACE`)
  rather than fall back to the default one, which would undo the isolation.
- `src/lib.rs`: re-exports the setter on Windows.

## Fifteenth patch: the embedder adjusts each webview's configuration (macOS) (Wis-original)

Some WebKit preferences only take effect when the webview is created, such as
the internal `PreferPageRenderingUpdatesNear60FPSEnabled` that holds pages to
60 fps on faster displays. Tauri builds the main window's webview from the app
config, so Wis could not hand it a configuration of its own.

- `src/wkwebview/mod.rs`: `wry::set_configuration_hook(Box<dyn Fn(&str, *mut
  c_void) + Send + Sync>)` (`OnceLock`, first registration wins), called on
  the main thread with the webview id and its `WKWebViewConfiguration` as a
  raw pointer (so the embedder's objc2 version need not match wry's), after
  wry's own setup and just before the `WKWebView` is created. It also runs
  for a configuration the embedder passed in.
- `src/lib.rs`: re-exports the setter on macOS and iOS.

## Sixteenth patch: the embedder hears of loads content rule lists block (macOS) (Wis-original)

WebKit reports each content rule list action through the private delegate
method `_webView:contentRuleListWithIdentifier:performedAction:forURL:`, and
only when the navigation delegate implements it when it is set, so the
embedder cannot add it to wry's delegate later. Wis counts the loads its own
lists block for the site panel.

- `src/wkwebview/mod.rs`: `wry::set_content_rule_block_hook(Box<dyn Fn(*mut
  c_void, &str, &str) + Send + Sync>)` (`OnceLock`, first registration wins),
  called on the main thread with the `WKWebView` as a raw pointer, the rule
  list's identifier and the blocked URL.
- `wry_navigation_delegate.rs` + `navigation.rs`: the delegate implements the
  method and calls the hook for actions whose `blockedLoad` is set, guarded by
  `respondsToSelector:`. The method sits in its own `impl` block: objc2 aborts
  at class registration when a protocol block holds a method the protocol does
  not declare.
- `src/lib.rs`: re-exports the setter on macOS and iOS.
