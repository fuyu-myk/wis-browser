// Where Tab moves focus in Wis's own UI (`lib/tabFocus.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { nextStop } from '../../src/lib/tabFocus.ts'

test('Tab moves forward and Shift+Tab back', () => {
  assert.equal(nextStop(4, 1, false), 2)
  assert.equal(nextStop(4, 1, true), 0)
})

test('the ends wrap round', () => {
  assert.equal(nextStop(4, 3, false), 0)
  assert.equal(nextStop(4, 0, true), 3)
})

test('with nothing focused Tab starts at the first stop and Shift+Tab at the last', () => {
  assert.equal(nextStop(4, -1, false), 0)
  assert.equal(nextStop(4, -1, true), 3)
})

test('with no stops there is nowhere to go', () => {
  assert.equal(nextStop(0, -1, false), -1)
})
