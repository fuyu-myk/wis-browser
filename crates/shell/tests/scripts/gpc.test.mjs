// `gpc.rs` GPC_JS: defines `navigator.globalPrivacyControl` with the baked setting, and `window.__wisGpcSet` to flip it.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'

const RAW = rawStringConst('gpc.rs', 'GPC_JS')
/** The script as `page_script(enabled)` bakes it. */
const scriptFor = (enabled) => RAW.replaceAll('%ON%', enabled ? 'true' : 'false')

// `flip_script` is a plain `format!` string, so it is read out directly.
const SOURCE = readFileSync(new URL('../../src-tauri/src/gpc.rs', import.meta.url), 'utf8')
const FLIP = SOURCE.match(/fn flip_script[\s\S]*?format!\("([^"]*)"\)/)[1]
const flipFor = (enabled) => FLIP.replace('{enabled}', String(enabled))

/** Runs the script in a fresh realm stand-in; returns its window, navigator and Navigator. */
function loadPage(enabled, { Navigator = class Navigator {}, navigator } = {}) {
  const nav = navigator ?? (Navigator ? new Navigator() : {})
  const window = {}
  const run = (on) => new Function('window', 'navigator', 'Navigator', scriptFor(on))(window, nav, Navigator)
  run(enabled)
  return { window, navigator: nav, Navigator, run, flip: (on) => new Function('window', flipFor(on))(window) }
}

describe('the baked signal', () => {
  test('navigator.globalPrivacyControl reports the setting baked in at creation', () => {
    assert.equal(loadPage(true).navigator.globalPrivacyControl, true)
    assert.equal(loadPage(false).navigator.globalPrivacyControl, false)
  })

  test('it is an enumerable, configurable getter on Navigator.prototype, as engines define it', () => {
    const p = loadPage(true)
    const d = Object.getOwnPropertyDescriptor(p.Navigator.prototype, 'globalPrivacyControl')
    assert.equal(typeof d.get, 'function')
    assert.equal(d.set, undefined)
    assert.equal(d.enumerable, true)
    assert.equal(d.configurable, true)
    assert.equal(Object.hasOwn(p.navigator, 'globalPrivacyControl'), false)
  })

  test('the page cannot assign it', () => {
    const p = loadPage(true)
    assert.equal(Reflect.set(p.navigator, 'globalPrivacyControl', false), false)
    assert.equal(p.navigator.globalPrivacyControl, true)
  })

  test('where the prototype refuses the property, the navigator itself gets it', () => {
    class Frozen {}
    Object.freeze(Frozen.prototype)
    const p = loadPage(true, { Navigator: Frozen })
    assert.equal(Object.hasOwn(p.navigator, 'globalPrivacyControl'), true)
    assert.equal(p.navigator.globalPrivacyControl, true)
  })

  test('with no Navigator interface at all, the navigator itself gets it', () => {
    const p = loadPage(true, { Navigator: null })
    assert.equal(p.navigator.globalPrivacyControl, true)
  })

  test('where both refuse, nothing throws and the setter is still installed', () => {
    let p
    assert.doesNotThrow(() => (p = loadPage(true, { Navigator: null, navigator: Object.freeze({}) })))
    assert.equal(typeof p.window.__wisGpcSet, 'function')
  })
})

describe('flipping the signal', () => {
  test('__wisGpcSet moves the signal, coerced to a boolean', () => {
    const p = loadPage(true)
    for (const [value, expected] of [
      [false, false],
      [1, true],
      ['', false],
      ['yes', true],
    ]) {
      p.window.__wisGpcSet(value)
      assert.equal(p.navigator.globalPrivacyControl, expected, String(value))
    }
  })

  test('__wisGpcSet is hidden from enumeration and cannot be replaced by assignment', () => {
    const p = loadPage(true)
    const d = Object.getOwnPropertyDescriptor(p.window, '__wisGpcSet')
    assert.equal(d.enumerable, false)
    assert.equal(d.writable, false)
    assert.equal(d.configurable, true)
  })

  test('running again in the same realm redefines both without throwing, and the newer one decides', () => {
    const p = loadPage(true)
    assert.doesNotThrow(() => p.run(false))
    assert.equal(p.navigator.globalPrivacyControl, false)
    p.window.__wisGpcSet(true)
    assert.equal(p.navigator.globalPrivacyControl, true)
  })

  test("the toggle's flip scripts move the baked signal, the last one winning", () => {
    const p = loadPage(true)
    p.flip(false)
    p.flip(true)
    p.flip(false)
    assert.equal(p.navigator.globalPrivacyControl, false)
  })

  test('a flip in a document without the property does nothing and does not throw', () => {
    assert.doesNotThrow(() => new Function('window', flipFor(true))({}))
  })
})
