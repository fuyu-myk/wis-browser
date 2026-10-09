// The readable part of a rejected command (`lib/errors.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { errorMessage } from '../../src/lib/errors.ts'

test('a string rejection is the message', () => {
  assert.equal(errorMessage('no webview labelled tab-1'), 'no webview labelled tab-1')
})

test('an object or Error with a string message gives that message', () => {
  assert.equal(errorMessage({ message: 'open a tab in this workspace first' }), 'open a tab in this workspace first')
  assert.equal(errorMessage(new Error('closed')), 'closed')
})

test('anything else is an unknown error, never "[object Object]"', () => {
  for (const value of [null, undefined, 42, {}, { message: 7 }]) {
    assert.equal(errorMessage(value), 'unknown error')
  }
})
