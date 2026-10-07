// The clear-data dialog's time ranges and site-data slices (`lib/clearData.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import {
  ALL_SITE_DATA_KINDS,
  CLEAR_RANGES,
  SITE_DATA_KIND_ROWS,
  clearSince,
} from '../../src/lib/clearData.ts'
import { readSource } from '../support/l3-harness.mjs'

const NOW_MS = 1_790_000_000_750

describe('how far back a clear reaches', () => {
  test('each range reaches back its span from the current unix second', () => {
    const now = Math.floor(NOW_MS / 1000)
    assert.equal(clearSince('hour', NOW_MS), now - 3600)
    assert.equal(clearSince('day', NOW_MS), now - 86_400)
    assert.equal(clearSince('week', NOW_MS), now - 604_800)
  })

  test('all time has no lower bound', () => {
    assert.equal(clearSince('all', NOW_MS), null)
  })

  test('the clock defaults to now', (t) => {
    t.mock.timers.enable({ apis: ['Date'], now: NOW_MS })
    assert.equal(clearSince('hour'), Math.floor(NOW_MS / 1000) - 3600)
  })

  test('the dialog offers the ranges from shortest to all time', () => {
    assert.deepEqual(
      CLEAR_RANGES.map((r) => r.value),
      ['hour', 'day', 'week', 'all'],
    )
    const bounds = CLEAR_RANGES.map((r) => clearSince(r.value, NOW_MS) ?? -Infinity)
    assert.deepEqual(bounds, [...bounds].sort((a, b) => b - a))
    assert.ok(CLEAR_RANGES.every((r) => r.label.trim() !== ''))
  })
})

describe('the slices of site data', () => {
  test('there is one labelled row per kind, in the same order', () => {
    assert.deepEqual(
      SITE_DATA_KIND_ROWS.map((r) => r.kind),
      ALL_SITE_DATA_KINDS,
    )
    assert.ok(SITE_DATA_KIND_ROWS.every((r) => r.label && r.hint))
  })

  test('the kinds are exactly the variants cookies.rs accepts', () => {
    const rust = readSource('src-tauri/src/cookies.rs')
    const at = rust.indexOf('pub enum SiteDataKind {')
    const body = rust.slice(at, rust.indexOf('}', at)).replace(/\/\/.*$/gm, '')
    assert.ok(rust.slice(0, at).trimEnd().endsWith('#[serde(rename_all = "snake_case")]'))
    const variants = [...body.matchAll(/^\s*([A-Z]\w*),?\s*$/gm)].map((m) => m[1].toLowerCase())
    assert.deepEqual([...ALL_SITE_DATA_KINDS].sort(), variants.sort())
  })
})
