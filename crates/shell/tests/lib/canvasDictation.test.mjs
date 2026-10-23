// Spoken prompts on the canvas (`features/canvas/lib/canvasDictation.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import * as dictation from '../../src/features/canvas/lib/canvasDictation.ts'

describe('what dictation writes into the prompt', () => {
  test('heard words follow what was typed, with one space between', () => {
    assert.equal(dictation.dictated('', 'make a timeline'), 'make a timeline')
    assert.equal(dictation.dictated('Plan', 'the launch'), 'Plan the launch')
    assert.equal(dictation.dictated('Plan  \t', 'the launch'), 'Plan the launch')
    assert.equal(dictation.dictated('Line one\n', 'line two'), 'Line one\nline two')
    assert.equal(dictation.dictated('Plan', ''), 'Plan')
  })
})

describe('events from Rust', () => {
  test('only well-formed events are read', () => {
    assert.deepEqual(dictation.readMicEvent({ type: 'heard', text: 'hi', done: false }), { type: 'heard', text: 'hi', done: false })
    assert.deepEqual(dictation.readMicEvent({ type: 'failed', message: 'No mic' }), { type: 'failed', message: 'No mic' })
    assert.deepEqual(dictation.readMicEvent({ type: 'ended' }), { type: 'ended' })
    assert.equal(dictation.readMicEvent({ type: 'heard', text: 5 }), null)
    assert.equal(dictation.readMicEvent({ type: 'heard', text: 'x', done: 'yes' }), null)
    assert.equal(dictation.readMicEvent({ type: 'failed' }), null)
    assert.equal(dictation.readMicEvent({ type: 'sing' }), null)
    assert.equal(dictation.readMicEvent(null), null)
  })

  test('heard text is capped to what a prompt can hold', () => {
    const long = dictation.readMicEvent({ type: 'heard', text: 'x'.repeat(50_000), done: true })
    assert.equal(long.text.length, dictation.MAX_HEARD)
  })
})
