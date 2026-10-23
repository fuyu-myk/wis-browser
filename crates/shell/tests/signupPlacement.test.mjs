// Where a password card sits beside its field (`features/vault/lib/signupPlacement.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { fieldInWindow, placeOffer } from '../src/features/vault/lib/signupPlacement.ts'

const pane = { x: 100, y: 50, width: 800, height: 600 }
const card = { width: 340, height: 190 }
const GAP = 6
const place = (field, within = pane) => placeOffer(field, within, card, GAP)

test('below the field, left-aligned with it', () => {
  assert.deepEqual(place({ x: 200, y: 200, width: 300, height: 40 }), { x: 200, y: 246, ...card })
})

test('above the field when it sits near the bottom', () => {
  assert.deepEqual(place({ x: 200, y: 560, width: 300, height: 40 }), { x: 200, y: 364, ...card })
})

test('pulled back inside the pane near its right edge', () => {
  assert.deepEqual(place({ x: 800, y: 200, width: 80, height: 40 }), { x: 560, y: 246, ...card })
})

test('no card once the field is scrolled out of the pane', () => {
  assert.equal(place({ x: 200, y: 0, width: 300, height: 40 }), null)
  assert.equal(place({ x: 200, y: 700, width: 300, height: 40 }), null)
})

test('pinned to the left edge of a pane narrower than the card', () => {
  const narrow = { x: 100, y: 50, width: 200, height: 600 }
  assert.equal(place({ x: 120, y: 100, width: 50, height: 30 }, narrow).x, 100)
})

test('the roomier side when neither fits, held inside the pane', () => {
  const short = { x: 100, y: 50, width: 800, height: 300 }
  assert.deepEqual(place({ x: 200, y: 140, width: 300, height: 40 }, short), { x: 200, y: 160, ...card })
})

test('a zoomed page is mapped through the measured scale', () => {
  assert.deepEqual(fieldInWindow({ x: 400, y: 300, width: 200, height: 60 }, 1600, pane), {
    x: 300,
    y: 200,
    width: 100,
    height: 30,
  })
})

test('an unknown viewport width is not scaled', () => {
  assert.deepEqual(fieldInWindow({ x: 10, y: 20, width: 30, height: 40 }, 0, pane), {
    x: 110,
    y: 70,
    width: 30,
    height: 40,
  })
})
