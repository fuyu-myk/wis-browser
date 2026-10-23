// Whether this window's Peek overlay is open, the flag key actions check
// before acting on the page beneath it (`features/page-tools/peekState.ts`).

import assert from 'node:assert/strict'
import { beforeEach, test } from 'node:test'

import { isPeekOpen, setPeekOpen } from '../../src/features/page-tools/peekState.ts'

const openAtLoad = isPeekOpen()

beforeEach(() => setPeekOpen(false))

test('a window starts with no peek open', () => {
  assert.equal(openAtLoad, false)
})

test('opening and closing the peek is reflected immediately', () => {
  setPeekOpen(true)
  assert.equal(isPeekOpen(), true)
  setPeekOpen(false)
  assert.equal(isPeekOpen(), false)
})

test('repeating the same state changes nothing', () => {
  setPeekOpen(true)
  setPeekOpen(true)
  assert.equal(isPeekOpen(), true)
  setPeekOpen(false)
  setPeekOpen(false)
  assert.equal(isPeekOpen(), false)
})
