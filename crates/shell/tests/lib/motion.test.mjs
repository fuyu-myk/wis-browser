// How much the chrome may move under each motion tier (`lib/motion.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { effectiveMotion, motionOff, motionStill, scrollBehavior } from '../../src/lib/motion.ts'

const body = (...classes) => ({ classList: new Set(classes) })

test('full motion moves and glides', () => {
  assert.equal(motionStill(body()), false)
  assert.equal(motionOff(body()), false)
  assert.equal(scrollBehavior(body()), 'smooth')
})

test('reduced motion keeps fades but nothing moves', () => {
  assert.equal(motionStill(body('motion-reduced')), true)
  assert.equal(motionOff(body('motion-reduced')), false)
  assert.equal(scrollBehavior(body('motion-reduced')), 'auto')
})

test('motion off stops everything', () => {
  assert.equal(motionStill(body('motion-off')), true)
  assert.equal(motionOff(body('motion-off')), true)
  assert.equal(scrollBehavior(body('motion-off')), 'auto')
})

test('the OS preference turns full motion into reduced, never into off', () => {
  assert.equal(effectiveMotion('full', true), 'reduced')
  assert.equal(effectiveMotion('full', false), 'full')
  assert.equal(effectiveMotion('reduced', false), 'reduced')
  assert.equal(effectiveMotion('off', true), 'off')
})
