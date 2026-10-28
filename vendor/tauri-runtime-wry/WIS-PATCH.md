# Vendored tauri-runtime-wry 2.11.2 (patched)

Verbatim copy of tauri-runtime-wry 2.11.2 from crates.io, applied via
`[patch.crates-io]` in the workspace `Cargo.toml`, with two patches.

One addition beyond the verbatim crate: a `rustfmt.toml` (copied from the
sibling vendored wry). The published crate ships none, so without it a
rustfmt-on-save tool would reformat the whole file to the 4-space default
instead of the crate's 2-space style. No effect on compilation.

## Patch: build Linux child webviews into `content_fixed`, not the vbox

- `src/lib.rs`, webview creation (`WebviewKind::WindowChild`, Linux arm): the
  runtime built every child webview into the window's default vertical
  `gtk::Box` via `build_gtk(window.default_vbox())`. A vertical box divides its
  height among its children, so positioned/overlapping child webviews (Wis's
  tabs, overlays, peek) stacked instead of overlaying, and `set_bounds` could
  not place them (tauri-apps/tauri#10420). The patch builds them into the
  window's `gtk::Fixed` overlay layer instead — `build_gtk(window.content_fixed())`
  — so wry positions them over the window and honors their bounds. The main
  content webview (`WebviewKind::WindowContent`) still uses `default_vbox` and
  is unchanged (it auto-fills via box packing).
- Depends on the `vendor/tao` patch (tao#1232) that adds
  `WindowExtUnix::content_fixed()`, and pairs with the `vendor/wry` patch
  (wry#1745) that makes `set_bounds` persist a child's position inside a
  `gtk::Fixed`. All three are required together; none fixes the bug alone.
- Pure GTK path — works on X11 and Wayland (unlike wry's X11-only
  `build_as_child` reparent route, which this deliberately does not use).

Applied **verbatim from the open upstream PR**
<https://github.com/tauri-apps/tauri/pull/15463> ("fix(linux): position
multi-webview children via gtk::Fixed"). That PR is a draft gated on a tao
release containing #1232; we carry all three vendored until they merge + ship.

Removal checklist: a tauri-runtime-wry release building children into
`content_fixed` exists (and the tao + wry fixes have shipped) -> delete
`vendor/tauri-runtime-wry`, `vendor/tao`, and the wry#1745 hunk, drop the three
`[patch.crates-io]` entries, `cargo update`, and re-test Linux multi-webview
positioning.

## Second patch: a webview can opt out of Tauri's scripts (Wis-original)

Tauri puts its own initialization scripts ahead of a webview's: `isTauri`,
`__TAURI_INTERNALS__` (with the window and webview labels), its IPC, events
and plugin scripts, including a macOS `window.print` that calls IPC. A webview
that loads remote pages cannot use any of them, since Tauri refuses its calls,
and they leave a page a way to tell the app apart.

- `src/lib.rs`: `SKIP_TAURI_SCRIPTS`, a marker script. When a webview's
  initialization scripts include it, the runtime drops it and every script
  before it, which are Tauri's when it is the webview's first own script.
  Without the marker nothing changes.

## Third patch: one ephemeral context per incognito data directory (Linux) (Wis-original)

The pair of wry's nineteenth patch. Webview contexts are stored by data
directory; an incognito webview that names one now creates that entry with
`WryWebContext::new_ephemeral()`, so every incognito webview naming the same
directory shares one in-memory context and nothing is written there. Without a
data directory an incognito webview still gets its own. Linux keeps stored
contexts for the app's lifetime (tauri#14626), so the embedder clears the
shared context's data when it is done with it.

- `src/lib.rs`: the vacant arm of `create_webview`'s context lookup.
