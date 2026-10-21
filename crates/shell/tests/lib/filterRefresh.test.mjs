// How far a filter list refresh has got (`lib/filterRefresh.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { FILTER_REFRESH_PROGRESS_EVENT, refreshProgressLine } from '../../src/lib/filterRefresh.ts'
import { readSource } from '../support/l3-harness.mjs'

test('names the list being fetched, then the apply step', () => {
  assert.equal(refreshProgressLine({ done: 0, total: 6 }), 'Fetching list 1 of 5...')
  assert.equal(refreshProgressLine({ done: 4, total: 6 }), 'Fetching list 5 of 5...')
  assert.equal(refreshProgressLine({ done: 5, total: 6 }), 'Applying to open tabs...')
})

test('listens for the event the refresh command sends', () => {
  const rust = readSource('src-tauri/src/commands/system.rs')
  assert.match(rust, new RegExp(`FILTER_REFRESH_PROGRESS_EVENT: &str = "${FILTER_REFRESH_PROGRESS_EVENT}"`))
})
