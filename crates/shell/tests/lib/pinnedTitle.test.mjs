// Where the Settings section titles drawn over the top strip go
// (`features/settings/lib/pinnedTitle.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { pinLayout } from '../../src/features/settings/lib/pinnedTitle.ts'

// The strip: a title pins at 12px, the fade covers the top 54px, titles are 30px tall.
const strip = { line: 12, band: 54, gap: 8 }
const tall = (n) => Array(n).fill(30)

test('at rest the first title is drawn where it is, nothing is pinned', () => {
  assert.deepEqual(pinLayout([32, 600], tall(2), strip), [{ index: 0, top: 32 }])
})

test('a title that reaches the line pins there', () => {
  assert.deepEqual(pinLayout([-200, 400], tall(2), strip), [{ index: 0, top: 12 }])
  assert.deepEqual(pinLayout([12, 400], tall(2), strip), [{ index: 0, top: 12 }])
})

test('the next title pushes the pinned one up as they meet', () => {
  // Pinned one is 30 tall at 12, plus the 8px gap: pushing starts at 50.
  assert.deepEqual(pinLayout([-200, 50], tall(2), strip), [{ index: 0, top: 12 }, { index: 1, top: 50 }])
  assert.deepEqual(pinLayout([-200, 40], tall(2), strip), [{ index: 0, top: 2 }, { index: 1, top: 40 }])
  assert.deepEqual(pinLayout([-200, 20], tall(2), strip), [{ index: 0, top: -18 }, { index: 1, top: 20 }])
})

test('once the next title is at the line it alone is pinned', () => {
  assert.deepEqual(pinLayout([-600, 12, 500], tall(3), strip), [{ index: 1, top: 12 }])
})

test('a title still below the strip is left to the page', () => {
  assert.deepEqual(pinLayout([-200, 80], tall(2), strip), [{ index: 0, top: 12 }])
  assert.deepEqual(pinLayout([], [], strip), [])
})
