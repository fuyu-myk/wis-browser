// How often streamed text repaints (`features/ai/lib/streamFlush.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { flushDue } from '../../src/features/ai/lib/streamFlush.ts'

const frames = (hz, count) => Array.from({ length: count }, (_, i) => 1000 + (i * 1000) / hz)

function flushes(times) {
  let last = -Infinity
  return times.filter((now) => {
    if (!flushDue(now, last)) return false
    last = now
    return true
  }).length
}

describe('flushDue', () => {
  test('flushes on every frame of a 60 Hz display', () => {
    assert.equal(flushes(frames(60, 60)), 60)
  })

  test('flushes on every other frame of a 120 Hz display', () => {
    assert.equal(flushes(frames(120, 120)), 60)
  })

  test('tolerates a 60 Hz frame that lands a little early', () => {
    assert.equal(flushDue(1015.5, 1000), true)
  })

  test('the first chunk flushes at once', () => {
    assert.equal(flushDue(5, -Infinity), true)
  })
})
