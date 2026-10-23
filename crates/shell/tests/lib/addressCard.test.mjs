// The saved-addresses card's size and what it says it fills (`features/vault/lib/addressCard.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { addressCardSize, fillsLine } from '../../src/features/vault/lib/addressCard.ts'

test('the card grows with its rows up to five, and holds at least one', () => {
  assert.deepEqual(addressCardSize(0), { width: 280, height: 104 })
  assert.deepEqual(addressCardSize(2), { width: 280, height: 140 })
  assert.deepEqual(addressCardSize(9), { width: 280, height: 248 })
})

test('the card names each kind of value a fill would reach, once', () => {
  assert.equal(fillsLine(['postal']), 'Fills postal code')
  assert.equal(fillsLine(['given', 'family', 'email']), 'Fills name and email')
  assert.equal(
    fillsLine(['name', 'line1', 'line2', 'city', 'postal', 'country', 'tel']),
    'Fills name, street address, city, postal code, country and phone',
  )
})
