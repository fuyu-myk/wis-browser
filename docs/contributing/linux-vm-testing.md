# Testing the Linux port in a VM

This guide covers running Wis on Linux inside a macOS-hosted VM to runtime-verify
the WebKitGTK port. It elaborates on `WIS.md`; if anything here conflicts,
`WIS.md` wins.

The Linux arms are compile-verified on CI (`ubuntu-22.04`) but never run, so a
VM is the cheapest way to exercise them from a macOS dev host.

## What a VM does and does not verify

A VM is a functional-correctness harness, not a rendering or performance one.

- Verified well: the ported logic and channel features (find, keybinds, cosmetic
  filtering, element picker, nav-guard, peek, open-in-tab, save-password prompt,
  media consent prompt) and, critically, page-capture pixel correctness (an
  endianness/byte-layout concern that is CPU, not GPU, so a VM exercises the
  exact path that matters).
- Verified poorly: compositing fidelity (overlay/webview stacking, glass and
  transparency under software GL), performance and lightness, and media actually
  streaming (needs webcam passthrough). For those, use real Linux hardware.

## 1. Create the VM (UTM)

UTM wraps QEMU with Apple's hypervisor and is far easier than raw `qemu`.

```bash
brew install --cask utm
```

### Get the ISO (arm64 images live on `cdimage.ubuntu.com`, not `releases.ubuntu.com`)

- Apple Silicon, easiest: Ubuntu 24.04 arm64 desktop,
  `https://cdimage.ubuntu.com/releases/24.04/release/ubuntu-24.04.4-desktop-arm64.iso`.
  Newer WebKitGTK (~2.44) than CI, so keep the render workaround in section 3
  handy. Functionally equivalent: the `webkit2gtk 2.0.2` crate targets the v2_30
  API, which both 2.36 and 2.44 provide.
- Apple Silicon, exact CI match: Ubuntu 22.04 arm64 has no desktop ISO, only
  server. Use
  `https://cdimage.ubuntu.com/releases/22.04/release/ubuntu-22.04.5-live-server-arm64.iso`,
  then after install run `sudo apt install -y ubuntu-desktop-minimal`.
- Intel Mac: Ubuntu 22.04 or 24.04 amd64 desktop from ubuntu.com.

### Steps in UTM

1. UTM, click the plus, Create a New Virtual Machine.
2. Choose Virtualize (not Emulate; Virtualize is hardware-accelerated for the
   native arch).
3. Choose Linux.
4. Leave "Use Apple Virtualization" unchecked (the QEMU backend gives the
   `virtio-gpu-gl` display WebKitGTK wants; Apple's backend has a more limited
   display). Under Boot ISO Image, browse to the ISO.
5. Memory 6144 to 8192 MB, CPU cores 4 or more.
6. Storage 40 to 64 GB.
7. Shared Directory, optional, pick a host folder to move files in and out.
8. Summary, name it `wis-linux`, Save.

Before first boot, open the VM settings (gear icon):

- Display, Emulated Display Card, choose `virtio-gpu-gl (GPU Supported)`. If the
  desktop later glitches or will not render, switch to `virtio-gpu-pci` and rely
  on software GL plus the env vars in section 3.
- Sharing, enable Clipboard Sharing (needed to verify capture to clipboard) and
  Directory Share.

### Install and integrate

1. Start the VM, run the Ubuntu installer. Minimal install is fine; enable
   third-party drivers and codecs for GPU and media. At the disk step choose
   "Erase disk and install Ubuntu", not "Install alongside": the disk is the
   VM's isolated virtual disk (not the Mac's), so erasing it is safe and gives
   Ubuntu the whole disk for build space.
2. When it finishes, shut down, then in the VM settings clear or eject the ISO
   from the CD/DVD drive so it boots from disk. Start again.
3. In the running guest, install the guest agents for clipboard and dynamic
   resolution, then reboot:

```bash
sudo apt update && sudo apt install -y spice-vdagent qemu-guest-agent
sudo reboot
```

## 2. Build Wis in the guest

System dependencies are the exact set CI installs, plus the toolchain basics a
fresh VM lacks:

```bash
sudo apt update
sudo apt install -y \
  build-essential curl git file pkg-config \
  libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev patchelf
```

Language toolchains:

```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
# Node and pnpm via nvm
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
# reopen the shell, then:
nvm install --lts && npm i -g pnpm
```

Build and run:

```bash
git clone https://github.com/fuyu-myk/wis-browser.git
cd wis-browser
git checkout linux-parity
cd crates/shell && pnpm install && cd ../..
make dev            # = pnpm tauri dev
```

The first build compiles the whole Rust workspace plus Tor, so it takes 10 to 30
minutes in a VM; give it the cores and RAM and let it run once. Tor is a default
cargo feature; if not testing Tor, `pnpm tauri dev -- --no-default-features`
trims a large chunk of the first build.

## 3. If pages render blank or black

Recent WebKitGTK uses a DMABUF renderer that fails under software GL in a VM,
showing a blank webview. This is a VM and driver quirk, not a bug in the port.

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 make dev
# still blank, add:
LIBGL_ALWAYS_SOFTWARE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 make dev
```

A related VM artifact: chrome elements that are **invisible but still
clickable** (the URL pill, nav circles, and other liquid-glass surfaces), and
webview dropdowns that open as fully transparent rectangles. Those are
accelerated-compositing layers (`backdrop-filter`, popup surfaces) that fail to
rasterize when GL is broken; layout and input still work, only the paint is
missing. Force WebKitGTK's non-composited path:

```bash
WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 make dev
```

If the elements appear with this set (or after toggling Liquid glass off in
Settings), the invisibility is the VM's software GL, not a port bug; verify
glass on real hardware.

The same flag has a flip side: webview transparency needs the compositing
path, so with it set, a transparent overlay webview (the downloads or
bookmarks popup) can punch through to the desktop while open, making the whole
browser look invisible until the popup closes. The two artifacts trade off
against each other under software GL; neither exists with working GL, so
treat both as VM-only and do the final visual verification on real hardware.

## 4. Runtime verification checklist

Keybinds below are the Linux keys. The app shows Cmd notation (macOS); on Linux
press Ctrl. Work top to bottom; capture is first because it is the one thing CI
cannot exercise.

### Page capture (highest priority: the ARGB32 conversion)

- Trigger: the Command Palette (search "Capture") or the capture control. Try
  Capture visible, Capture region (drag a rectangle), and Capture window.
- Correct: a PNG lands on the clipboard. Paste it into an image editor and check
  the colors are accurate: no red and blue channel swap, no ghosting or haloing
  around semi-transparent elements (that would mean the un-premultiply is wrong),
  and no vertical skew or offset (a stride bug). Region matches the dragged
  rectangle. Window is page-only with no chrome, the same documented degrade as
  Windows.
- Why first: `imp_linux::surface_to_png` converts cairo ARGB32 (premultiplied,
  native-endian, stride-padded) to straight RGBA. The logic is source-reasoned
  for both endiannesses but never run.

### Camera and microphone consent (security)

- Trigger: open a getUserMedia test page (for example
  `webrtc.github.io/samples/src/content/getusermedia/gum/`) and start the camera.
- Correct: the in-browser consent prompt appears before the camera activates.
  Granting starts the stream (requires a webcam passed to the VM via UTM USB
  passthrough); denying rejects with a `NotAllowedError`. With no webcam attached,
  the prompt should still appear (that is the security half); the stream simply
  has no device.
- Follow-up to confirm: the native `permission-request` bridge allows every
  `UserMediaPermissionRequest`, which in the 2.0.2 binding also covers
  `getDisplayMedia` screen capture. Confirm screen-share behaviour is acceptable
  (the OS portal still gates it on Wayland).

### Find in page

- Trigger: `Ctrl+F`, type a term, `Ctrl+G` and `Ctrl+Shift+G` to step matches.
- Correct: matches highlight, the view scrolls to the current match, and the
  FindBar shows a total count.
- Expected limitation, not a bug: no "3 of 12" current index. WebKitGTK has no
  match-index API, so the index is emitted as -1 and only the total shows.
- Follow-up to confirm: close the FindBar and re-search the same term. It may
  step to the next match rather than re-searching from the top (`search_text`
  persists past `search_finish`). Note the behaviour.

### Keyboard shortcuts

- Trigger, tab focused: `Ctrl+R` reload, `Ctrl+F` find, `Ctrl+=` and `Ctrl+-`
  zoom, `Ctrl+[` and `Ctrl+]` back and forward, `Ctrl+P` print.
- Correct: the shortcuts fire while a tab has focus.
- Follow-up to confirm: focus the chrome instead of a tab (click the URL bar or a
  sidebar), then press a global shortcut. The chrome-side `useKeybindCapture`
  hook is Windows-only, so chrome-focused shortcuts may not fire on Linux. If
  broken, that is the flagged follow-up (widen `useKeybindCapture` to Linux).

### Cosmetic filtering and element picker

- Trigger, automatic: visit an ad-heavy page in a non-Permissive workspace; ad
  slots and empty placeholders should be hidden.
- Trigger, picker: `Ctrl+Shift+X`, click an element to hide it, then reload.
- Correct: the picked element stays hidden after reload (persisted per host).
- Expected limitation, not a bug: cross-origin-iframe cosmetic is dropped
  (top-frame only), the same as Windows.

### Navigation-guard interstitial

- Trigger: click a risky link (an `http://` downgrade, or a suspicious pattern)
  with the fraudulent-website warning enabled.
- Correct: an interstitial warns before navigating; the back action dismisses or
  returns.

### Peek link preview

- Trigger: `Alt+click` a link (`Opt+click` on macOS).
- Correct: an isolated preview webview grows from the link and shows the page
  plus a safety assessment.
- Expected cosmetic gap: the preview has square corners (the 12px rounding is
  macOS and Windows only).

### Open link in a background tab

- Trigger: `Ctrl+click` or middle-click a link.
- Correct: opens in a new background tab in the current workspace.

### In-page drag and drop

- Trigger: open github.com/\<user\>. Customize your pins and drag a pinned repo to a new slot; then drag a file from Files onto a page `<input type="file">`.
- Correct: the pin reorders and Save enables; the file attaches. Wis disables wry's drag-drop handler on every webview, so WebKitGTK handles both itself.

### Save-password capture

- Trigger: submit a login form on any test login page (non-private tab).
- Correct: a "Save password?" prompt appears. Note that vault unlock is not
  ported (see section 6); only the capture prompt is under test here.

### Earlier waves (lighter pass)

- Zoom (`Ctrl+=` and `Ctrl+-`), tab audio mute (click the audio icon on a tab
  playing sound), print (`Ctrl+P`, a GTK print dialog), downloads (download a
  file and confirm it lands in the downloads surface), clipboard copy and paste,
  and per-workspace wallpapers and custom themes rendering.

## 5. Not ported (do not file as bugs)

These are gaps documented in `WIS.md`; they intentionally do not work on
Linux:

- Video pop-out: raises a "not available" toast. WebKitGTK has no
  Picture-in-Picture, and a cross-window reparent tears down compositing.
- Password vault biometric unlock: no portable Linux device-owner-auth gate.
- Per-tab memory in the task manager: shows empty. The shared-process model has
  no per-tab process id, so honest-empty is correct.

## 6. Known follow-ups to confirm or refute

Turning these from unknown into confirmed or fixed is the main value of a VM run:

- Capture ARGB32 pixel correctness (section 4, page capture).
- Find reopen-same-term stepping (section 4, find).
- Media bridge `getDisplayMedia` breadth (section 4, consent).
- Chrome-focused keyboard shortcuts (section 4, shortcuts).
- Clipboard under X11 versus Wayland: test both by picking the session type at
  the login screen (the gear icon), since `arboard` behaves differently across
  them.

## Reporting

The `linux-parity` branch is compile-verified only and is not merged to `main`
until at least a smoke pass here. File findings against that branch and note
which of the follow-ups above were confirmed, fixed, or refuted.
