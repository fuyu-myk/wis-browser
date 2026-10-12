// Matching and ranking for the settings search (`lib/settingsSearch.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { searchSettings } from '../../src/lib/settingsSearch.ts'

const ENTRIES = [
  { section: 'Privacy', title: 'Count time on sites', detail: 'On-device tally of active time per site.' },
  { section: 'Privacy', title: 'Week starts on', detail: 'For the Usage panel and page.' },
  { section: 'Appearance', title: 'Theme', detail: 'Light, dark, or follow system' },
  { section: 'Network', title: 'Upstream proxy', detail: 'Route this workspace through a proxy.' },
  { section: 'Appearance', title: 'Time format', detail: '12 or 24 hour clock.' },
]

test('an empty query matches nothing', () => {
  assert.deepEqual(searchSettings(ENTRIES, ''), [])
  assert.deepEqual(searchSettings(ENTRIES, '   '), [])
})

test('every word must appear in the title, detail or section', () => {
  assert.deepEqual(searchSettings(ENTRIES, 'usage week'), [1])
  assert.deepEqual(searchSettings(ENTRIES, 'NETWORK'), [3])
  assert.deepEqual(searchSettings(ENTRIES, 'proxy missing'), [])
})

test('titles that start with the query rank first, then title matches', () => {
  assert.deepEqual(searchSettings(ENTRIES, 'time'), [4, 0])
  assert.deepEqual(searchSettings(ENTRIES, 'theme'), [2])
  assert.deepEqual(searchSettings(ENTRIES, 'privacy'), [0, 1])
  assert.deepEqual(searchSettings(ENTRIES, 'site'), [0])
})

test('results stop at the limit', () => {
  assert.deepEqual(searchSettings(ENTRIES, 'e', 2).length, 2)
})
