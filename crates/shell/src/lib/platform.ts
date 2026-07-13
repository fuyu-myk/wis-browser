/**
 * Synchronous host-OS detection for the chrome webview.
 *
 * The platform-specific default keymap and accelerator glyphs are decided here.
 *
 * Detection reads `navigator` because the local chrome webview runs on the
 * host engine (WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux)
 * and its user agent is left untouched (only remote tab/Peek webviews get the
 * Safari-compat UA), so it reflects the real OS. `navigator.userAgentData` is
 * unavailable in WKWebView, so the legacy `userAgent` token is the only signal
 * that works across all three engines.
 */

export type Platform = 'macos' | 'windows' | 'linux'

function detect(): Platform {
  const ua = typeof navigator !== 'undefined' ? navigator.userAgent : ''
  if (/Windows|Win32|Win64/i.test(ua)) return 'windows'
  if (/Macintosh|Mac OS X/i.test(ua)) return 'macos'
  if (/Linux|X11/i.test(ua)) return 'linux'
  // Unknown engines fall back to macOS, matching the Rust UA/keymap default.
  return 'macos'
}

// Resolved once: the host OS does not change during a session.
const PLATFORM: Platform = detect()

/** The host platform of the running build. */
export function currentPlatform(): Platform {
  return PLATFORM
}

/** True on macOS, where accelerators use `Cmd` and render as glyphs. */
export function isMacOS(): boolean {
  return PLATFORM === 'macos'
}

/**
 * Whether a hidden or pending webview may be parked at an off-screen coordinate.
 * Linux composites every child webview into one shared gtk::Fixed, so parking a
 * child at a huge coordinate balloons the Fixed and breaks the window layout;
 * on Linux, hide via visibility instead. macOS (separate NSViews) and Windows
 * (separate HWNDs) can park off-screen safely.
 */
export function canParkOffscreen(): boolean {
  return PLATFORM !== 'linux'
}
