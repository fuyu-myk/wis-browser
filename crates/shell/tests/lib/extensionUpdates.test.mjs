// What a check for extension updates reports (`lib/extensionUpdates.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { describeUpdates } from '../../src/lib/extensionUpdates.ts'

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
