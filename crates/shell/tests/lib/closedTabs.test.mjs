// The palette reaches every closed tab the backend keeps.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { CLOSED_TABS_CHANGED_EVENT, visibleClosedTabs } from '../../src/features/history/closedTabs.ts'
import { CLOSED_TAB_LIMIT } from '../../src/lib/palette.ts'
import { readSource } from '../support/l3-harness.mjs'

const tab = (id, workspace, title, url) => ({
  id,
  workspace_id: workspace,
  url,
  title,
  folder_id: null,
  position: 0,
  pinned: false,
  closed_at: '2026-10-21T10:00:00Z',
  archived: false,
})
const tabs = [
  tab(1, 'ws-a', 'Rust book', 'https://doc.rust-lang.org/book/'),
  tab(2, 'ws-b', 'Weather', 'https://weather.example/today'),
  tab(3, 'ws-a', '', 'https://news.example/story'),
]

test('a workspace sees only its own closed tabs, and all of them when unscoped', () => {
  assert.deepEqual(visibleClosedTabs(tabs, 'ws-a', '').map((t) => t.id), [1, 3])
  assert.deepEqual(visibleClosedTabs(tabs, null, '').map((t) => t.id), [1, 2, 3])
})

test('a search matches title or address, ignoring case', () => {
  assert.deepEqual(visibleClosedTabs(tabs, null, ' RUST ').map((t) => t.id), [1])
  assert.deepEqual(visibleClosedTabs(tabs, null, 'news.example').map((t) => t.id), [3])
  assert.deepEqual(visibleClosedTabs(tabs, 'ws-b', 'rust'), [])
})

test('the page hears the event the backend sends when the list changes', () => {
  const rust = readSource('src-tauri/src/commands/closed_tabs.rs')
  assert.match(rust, new RegExp(`CLOSED_TABS_CHANGED_EVENT: &str = "${CLOSED_TABS_CHANGED_EVENT}"`))
})

test('the palette asks for as many closed tabs as Recently closed keeps', () => {
  const rust = readSource('src-tauri/src/commands/tab.rs')
  assert.match(rust, new RegExp(`pub const CLOSED_TAB_CAP: u32 = ${CLOSED_TAB_LIMIT};`))
})
