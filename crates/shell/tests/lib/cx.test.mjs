// The class-name joiner (`lib/cx.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { cx } from '../../src/lib/cx.ts'

test('strings join with single spaces', () => {
  assert.equal(cx('tab', 'is-active'), 'tab is-active')
})

test('false, null, undefined and empty strings are skipped', () => {
  assert.equal(cx('tab', false, null, undefined, '', 'pinned'), 'tab pinned')
  assert.equal(cx(), '')
})

test('an object contributes the keys whose condition is truthy, in order', () => {
  const flags = { 'is-active': true, compact: false, muted: null, loud: undefined, pinned: true }
  assert.equal(cx('tab', flags), 'tab is-active pinned')
})

test('nested arrays flatten, and an array that yields nothing adds no space', () => {
  assert.equal(cx('a', ['b', ['c', { d: true }]], [false, { e: false }], 'f'), 'a b c d f')
})
