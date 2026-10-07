// Reordering launchpad shortcut tiles after a drag (`lib/shortcuts.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { reorderByKeys } from '../../src/lib/shortcuts.ts'

const a = { label: 'A', url: 'https://a.test/' }
const b = { label: 'B', url: 'https://b.test/' }
const c = { label: 'C', url: 'https://c.test/' }

test('tiles follow the dragged order of their URLs', () => {
  assert.deepEqual(reorderByKeys([a, b, c], [c.url, a.url, b.url]), [c, a, b])
})

test('a tile missing from the order is dropped', () => {
  assert.deepEqual(reorderByKeys([a, b, c], [b.url, a.url]), [b, a])
})

test('an unknown URL in the order is ignored', () => {
  assert.deepEqual(reorderByKeys([a, b], ['https://gone.test/', b.url, a.url]), [b, a])
})

test('the tiles themselves are returned, not copies, and the input is left alone', () => {
  const tiles = [a, b]
  const out = reorderByKeys(tiles, [b.url, a.url])
  assert.equal(out[0], b)
  assert.deepEqual(tiles, [a, b])
})

test('no tiles or no order gives an empty list', () => {
  assert.deepEqual(reorderByKeys([], [a.url]), [])
  assert.deepEqual(reorderByKeys([a], []), [])
})
