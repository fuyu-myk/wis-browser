// Event names shared by the widget-panel overlays and the chrome
// (`lib/widgetResize.ts`).

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

import * as events from '../../src/lib/widgetResize.ts'

test('every widget-panel event has its own name in the wis namespace', () => {
  const names = Object.entries(events)
    .filter(([key]) => key.endsWith('_EVENT'))
    .map(([, name]) => name)
  assert.equal(names.length, 7)
  assert.equal(new Set(names).size, names.length)
  for (const name of names) assert.match(name, /^wis-w(resize|panel)-[a-z]+$/)
})

test('the panel navigation event matches the name the panel webview emits', () => {
  const rust = readFileSync(new URL('../../src-tauri/src/chrome/widget_panel.rs', import.meta.url), 'utf8')
  const emitted = rust.match(/const WPANEL_NAV_EVENT: &str = "([^"]+)";/)[1]
  assert.equal(events.WPANEL_NAV_EVENT, emitted)
})
