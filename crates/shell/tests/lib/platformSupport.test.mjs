// Which features each platform lacks, and the toast that says so
// (`lib/platformSupport.ts`).

import assert from 'node:assert/strict'
import { beforeEach, describe, test } from 'node:test'

import { inFreshProcess, readSource, srcUrl } from '../support/l3-harness.mjs'

const UA = {
  macos: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)',
  windows: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/129.0.0.0 Edg/129.0.0.0',
  linux: 'Mozilla/5.0 (X11; Ubuntu; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)',
}
const FEATURES = ['pop_out_video', 'ai_assistant']

// The platform is fixed when `lib/platform` loads, so this process runs as
// Linux and the other platforms are probed in fresh processes.
Object.defineProperty(globalThis, 'navigator', { value: { userAgent: UA.linux }, configurable: true })
globalThis.window = new EventTarget()
const { PLATFORM_LABEL, guardFeatureUnavailable, isFeatureUnavailableHere } = await import(
  '../../src/lib/platformSupport.ts'
)
const { TOAST_REQUEST_EVENT } = await import('../../src/lib/toast.ts')

let toasts = []
window.addEventListener(TOAST_REQUEST_EVENT, (e) => toasts.push(e.detail.message))
beforeEach(() => {
  toasts = []
})

/** What the feature checks report on `platform`, from a fresh process. */
function probe(platform, features = FEATURES) {
  return inFreshProcess(
    async (_harness, { userAgent, module, features }) => {
      Object.defineProperty(globalThis, 'navigator', { value: { userAgent } })
      globalThis.window = new EventTarget()
      const toasts = []
      window.addEventListener('wis-toast-request', (e) => toasts.push(e.detail.message))
      const support = await import(module)
      return {
        unavailable: features.filter((id) => support.isFeatureUnavailableHere(id)),
        guarded: features.filter((id) => support.guardFeatureUnavailable(id)),
        toasts,
      }
    },
    { userAgent: UA[platform], module: srcUrl('lib/platformSupport.ts'), features },
  )
}

test('every platform has the display name the toast uses', () => {
  assert.deepEqual(PLATFORM_LABEL, { macos: 'macOS', windows: 'Windows', linux: 'Linux' })
})

describe('on Linux', () => {
  test('the video pop-out and the AI assistant are unavailable', () => {
    assert.deepEqual(FEATURES.filter(isFeatureUnavailableHere), FEATURES)
  })

  test('guarding an unavailable feature raises its toast and reports it handled', () => {
    assert.equal(guardFeatureUnavailable('pop_out_video'), true)
    assert.equal(guardFeatureUnavailable('ai_assistant'), true)
    assert.deepEqual(toasts, [
      'Video pop-out not available on Linux',
      'AI assistant not available on Linux',
    ])
  })

  test('checking availability alone raises no toast', () => {
    isFeatureUnavailableHere('pop_out_video')
    assert.deepEqual(toasts, [])
  })
})

test('an unknown feature id is never reported unavailable and raises no toast', () => {
  assert.equal(isFeatureUnavailableHere('new_tab'), false)
  assert.equal(guardFeatureUnavailable('new_tab'), false)
  assert.deepEqual(toasts, [])
})

for (const platform of ['macos', 'windows']) {
  test(`on ${PLATFORM_LABEL[platform]} every feature is available and the guard stays silent`, () => {
    assert.deepEqual(probe(platform), { unavailable: [], guarded: [], toasts: [] })
  })
}

test('the AI assistant is offered exactly where ai.rs can seal its key', () => {
  const rust = readSource('src-tauri/src/ai.rs')
  const at = rust.indexOf('fn ai_supported()')
  const body = rust.slice(at, rust.indexOf('}', at))
  const rustSupports = {
    macos: body.includes('target_os = "macos"'),
    windows: /\bwindows\b/.test(body),
    linux: body.includes('target_os = "linux"') || /\bunix\b/.test(body),
  }
  assert.deepEqual(rustSupports, { macos: true, windows: true, linux: false })
  assert.equal(isFeatureUnavailableHere('ai_assistant'), true, 'Linux')
})

test('the JavaScript switch is offered exactly where page_preferences.rs sets it per page', () => {
  const rust = readSource('src-tauri/src/page_preferences.rs')
  assert.match(rust, /#\[cfg\(target_os = "macos"\)\]\npub fn install_hook/)
  assert.equal(isFeatureUnavailableHere('site_javascript'), true, 'Linux')
  assert.deepEqual(probe('windows', ['site_javascript']).unavailable, ['site_javascript'])
  assert.deepEqual(probe('macos', ['site_javascript']).unavailable, [])
})

test('blocked counts are offered exactly where page_blocks.rs hears of blocks', () => {
  const rust = readSource('src-tauri/src/page_blocks.rs')
  assert.match(rust, /#\[cfg\(target_os = "macos"\)\]\npub fn install/)
  assert.equal(isFeatureUnavailableHere('site_block_counts'), true, 'Linux')
  assert.deepEqual(probe('windows', ['site_block_counts']).unavailable, ['site_block_counts'])
  assert.deepEqual(probe('macos', ['site_block_counts']).unavailable, [])
})

test('Lockdown Mode is offered exactly where hardening.rs applies it', () => {
  const rust = readSource('src-tauri/src/hardening.rs')
  assert.match(rust, /#\[cfg\(target_os = "macos"\)\]\npub unsafe fn apply/)
  assert.equal(isFeatureUnavailableHere('lockdown_mode'), true, 'Linux')
  assert.deepEqual(probe('windows', ['lockdown_mode']).unavailable, ['lockdown_mode'])
  assert.deepEqual(probe('macos', ['lockdown_mode']).unavailable, [])
})

test('the fingerprinting switch is offered exactly where WebKit takes it per page', () => {
  const rust = readSource('src-tauri/src/site_protection.rs')
  assert.match(rust, /#\[cfg\(target_os = "macos"\)\]\npub mod imp \{[\s\S]*pub unsafe fn set_fingerprinting_protection/)
  assert.equal(isFeatureUnavailableHere('site_fingerprinting'), true, 'Linux')
  assert.deepEqual(probe('windows', ['site_fingerprinting']).unavailable, ['site_fingerprinting'])
  assert.deepEqual(probe('macos', ['site_fingerprinting']).unavailable, [])
})
