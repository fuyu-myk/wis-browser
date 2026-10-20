// What a check for extension updates reports (`lib/extensionUpdates.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { describeLinked, describeUpdates } from '../../src/lib/extensionUpdates.ts'

const none = { updated: [], waiting: [], failed: [] }

describe('describeUpdates', () => {
  test('says when nothing changed', () => {
    assert.equal(describeUpdates(none), 'Everything is up to date.')
  })

  test('names what was updated, what waits and what failed', () => {
    assert.equal(describeUpdates({ ...none, updated: ['Dark Reader'] }), 'Updated Dark Reader.')
    assert.equal(
      describeUpdates({ updated: ['A', 'B'], waiting: ['C'], failed: ['D', 'E', 'F'] }),
      'Updated A and B. C waits for your approval below. D, E and F could not be checked.',
    )
  })
})

describe('describeLinked', () => {
  test('says what linking did', () => {
    assert.equal(describeLinked('Dark Reader', 'current'), 'Dark Reader now gets its updates from the Chrome Web Store.')
    assert.equal(describeLinked('Dark Reader', 'updated'), 'Dark Reader now gets its updates from the Chrome Web Store, and was just updated.')
    assert.equal(
      describeLinked('Dark Reader', 'waiting'),
      'Dark Reader now gets its updates from the Chrome Web Store. Its update asks for more, and awaits approval in Settings.',
    )
  })
})
