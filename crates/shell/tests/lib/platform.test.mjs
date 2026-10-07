// Host OS detection from the chrome webview's user agent (`lib/platform.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { inFreshProcess, srcUrl } from '../support/l3-harness.mjs'

// What each host engine reports for the local chrome webview.
const UA = {
  wkwebview: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)',
  webview2:
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36 Edg/129.0.0.0',
  webkitgtk:
    'Mozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15',
}

/**
 * What the module reports when it loads under `navigator` (undefined for
 * none), optionally after the user agent changes to `later`. The platform is
 * resolved once at load, so each case runs in a fresh process.
 */
function profileOn(navigator, later) {
  return inFreshProcess(
    async (_harness, arg) => {
      const setNavigator = (value) =>
        Object.defineProperty(globalThis, 'navigator', { value, configurable: true })
      setNavigator(arg.navigator)
      const p = await import(arg.module)
      if (arg.later) setNavigator({ userAgent: arg.later })
      return { platform: p.currentPlatform(), mac: p.isMacOS(), parksOffscreen: p.canParkOffscreen() }
    },
    { module: srcUrl('lib/platform.ts'), navigator, later },
  )
}

describe('each host engine maps to its platform', () => {
  test('a WKWebView user agent is macOS, with Cmd accelerators and off-screen parking', () => {
    assert.deepEqual(profileOn({ userAgent: UA.wkwebview }), {
      platform: 'macos',
      mac: true,
      parksOffscreen: true,
    })
  })

  test('a WebView2 user agent is Windows, which can park webviews off-screen', () => {
    assert.deepEqual(profileOn({ userAgent: UA.webview2 }), {
      platform: 'windows',
      mac: false,
      parksOffscreen: true,
    })
  })

  test('a WebKitGTK user agent is Linux, which must not park webviews off-screen', () => {
    assert.deepEqual(profileOn({ userAgent: UA.webkitgtk }), {
      platform: 'linux',
      mac: false,
      parksOffscreen: false,
    })
  })
})

test('the platform tokens are matched case-insensitively', () => {
  assert.equal(profileOn({ userAgent: 'something WIN64 something' }).platform, 'windows')
})

test('an unrecognised engine falls back to macOS, matching the Rust default', () => {
  assert.deepEqual(profileOn({ userAgent: 'Node.js/24' }), {
    platform: 'macos',
    mac: true,
    parksOffscreen: true,
  })
})

test('a missing navigator also falls back to macOS', () => {
  assert.equal(profileOn(undefined).platform, 'macos')
})

test('the platform is fixed at load and ignores a later user agent', () => {
  assert.deepEqual(profileOn({ userAgent: UA.webkitgtk }, UA.webview2), {
    platform: 'linux',
    mac: false,
    parksOffscreen: false,
  })
})
