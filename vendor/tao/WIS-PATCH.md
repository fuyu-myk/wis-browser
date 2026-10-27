# Vendored tao 0.35.2 (patched)

Verbatim copy of tao 0.35.2 from crates.io, applied via `[patch.crates-io]`
in the workspace `Cargo.toml`, with one feature added and one macOS input fix.

One addition beyond the verbatim crate: a `rustfmt.toml` (copied from the
sibling vendored wry, which ships one; the published tao crate does not). It
matches tao's upstream repo style (`tab_spaces = 2`) so an accidental edit
through a rustfmt-on-save tool reformats only the touched region instead of
converting the whole file to the rustfmt default (4-space). It has no effect
on compilation.

## Patch: `content_fixed` overlay layer for positioned child webviews (Linux)

- `src/platform_impl/linux/window.rs` + `src/platform/unix.rs`: on Linux, tao
  gave each window a single vertical `gtk::Box` (`default_vbox`) as its content
  container, and a runtime (tauri-runtime-wry) packed every webview into it.
  A vertical box *divides* its height among its children, so multiple child
  webviews stack/split instead of overlapping, and `WebView::set_bounds` cannot
  position them (tauri-apps/tauri#10420; the root cause of Wis overlays/tabs/
  peek rendering below their rectangles on Linux). The patch wraps the content
  in a `gtk::Overlay`: the `default_vbox` stays the overlay's main child (so
  single-webview windows are unchanged), and a full-window pass-through
  `gtk::Fixed` is added as an overlay layer, exposed via
  `WindowExtUnix::content_fixed()`. A runtime builds positioned child webviews
  into that `gtk::Fixed` so they overlay the window and honor their bounds.
  `set_overlay_pass_through(&fixed, true)` lets input fall through the empty
  areas of the Fixed to the content below, while the webviews inside it still
  receive input.
- Pure GTK, no X11/Wayland-specific API, works on both backends (unlike wry's
  X11-only `build_as_child` reparent path).

Applied **verbatim from the open upstream PR**
<https://github.com/tauri-apps/tao/pull/1232> ("feat(linux): add
`content_fixed` overlay layer for positioned child webviews"). Pairs with
`vendor/tauri-runtime-wry` (tauri#15463, routes children into `content_fixed`)
and `vendor/wry` (wry#1745, makes `set_bounds` persist in a `gtk::Fixed`).

Removal checklist: a tao release containing #1232 exists -> delete `vendor/tao`
and the `[patch.crates-io]` entry, `cargo update -p tao`, and re-test Linux
multi-webview positioning (tabs/overlays/peek land on their rectangles).

## Patch: no stray characters from keys a child webview declined (macOS)

- `src/platform_impl/macos/view.rs`, `key_down`: tao's content view ran
  `interpretKeyEvents:` on every key it received. Wis's webviews are child
  views of that content view, so a key a focused webview does not use (Left
  at the start of an empty field, Right at its end, Up/Down, Home/End at the
  edges) travels up the responder chain to it, and interpreting it again
  typed the key's private-use character (U+F702, U+F703 and so on, drawn as
  boxes) into that webview's field, in Wis's own fields and in web pages
  alike. The patch interprets only when the content view itself is the
  window's first responder, the same check tao's `insert_tab` uses. The
  `KeyboardInput` window event is still queued for every key.
- Upstream: tauri-apps/tauri#10194 (open). wry#769 once guarded arrow keys on
  the parent view; wry#798 removed that path.

Removal checklist: upstream tao or wry fixes tauri#10194 -> drop this hunk,
and check that Left/Right in an empty field types nothing.
