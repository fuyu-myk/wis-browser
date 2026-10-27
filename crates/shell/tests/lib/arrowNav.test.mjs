// Where the arrow keys move focus in a menu, list or group (`lib/arrowNav.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { arrowStep } from '../../src/lib/arrowNav.ts'

const none = [false, false, false, false]

test('down and up move through a vertical list, wrapping', () => {
  assert.equal(arrowStep('ArrowDown', 'vertical', 1, none), 2)
  assert.equal(arrowStep('ArrowDown', 'vertical', 3, none), 0)
  assert.equal(arrowStep('ArrowUp', 'vertical', 0, none), 3)
})

test('left and right drive a horizontal group, and are ignored by a vertical one', () => {
  assert.equal(arrowStep('ArrowRight', 'horizontal', 0, none), 1)
  assert.equal(arrowStep('ArrowLeft', 'horizontal', 0, none), 3)
  assert.equal(arrowStep('ArrowRight', 'vertical', 0, none), null)
  assert.equal(arrowStep('ArrowDown', 'horizontal', 0, none), null)
})

test('a group in both directions answers all four arrows', () => {
  assert.equal(arrowStep('ArrowDown', 'both', 0, none), 1)
  assert.equal(arrowStep('ArrowLeft', 'both', 0, none), 3)
})

test('Home and End go to the first and last enabled item', () => {
  const ends = [true, false, false, true]
  assert.equal(arrowStep('Home', 'vertical', 2, ends), 1)
  assert.equal(arrowStep('End', 'vertical', 1, ends), 2)
})

test('disabled items are skipped', () => {
  assert.equal(arrowStep('ArrowDown', 'vertical', 0, [false, true, true, false]), 3)
})

test('with nothing focused, down starts at the top and up at the bottom', () => {
  assert.equal(arrowStep('ArrowDown', 'vertical', -1, none), 0)
  assert.equal(arrowStep('ArrowUp', 'vertical', -1, none), 3)
})

test('other keys, empty or all-disabled groups go nowhere', () => {
  assert.equal(arrowStep('Enter', 'vertical', 0, none), null)
  assert.equal(arrowStep('ArrowDown', 'vertical', -1, []), null)
  assert.equal(arrowStep('ArrowDown', 'vertical', 0, [true, true]), null)
})
