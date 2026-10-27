// Keyboard resizing of a split-pane divider (`lib/paneDivider.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { dividerStep } from '../../src/lib/paneDivider.ts'

const limits = { min: 150, max: 850 }

test('the arrows along the divider\'s axis move it by a twentieth', () => {
  assert.equal(dividerStep('ArrowLeft', true, 500, limits), 450)
  assert.equal(dividerStep('ArrowRight', true, 500, limits), 550)
  assert.equal(dividerStep('ArrowUp', false, 500, limits), 450)
  assert.equal(dividerStep('ArrowDown', false, 500, limits), 550)
})

test('arrows across the axis do nothing', () => {
  assert.equal(dividerStep('ArrowUp', true, 500, limits), null)
  assert.equal(dividerStep('ArrowLeft', false, 500, limits), null)
})

test('Home and End go to the limits, and steps stop there', () => {
  assert.equal(dividerStep('Home', true, 500, limits), 150)
  assert.equal(dividerStep('End', false, 500, limits), 850)
  assert.equal(dividerStep('ArrowLeft', true, 170, limits), 150)
  assert.equal(dividerStep('ArrowRight', true, 850, limits), null)
})
