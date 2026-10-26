// What the saved-cards card at a card field shows (`features/vault/lib/cardOffer.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { cardFillsLine, cardRows } from '../../src/features/vault/lib/cardOffer.ts'

test('the note names what a fill reaches, never more', () => {
  assert.equal(cardFillsLine(['number', 'exp']), 'Fills card number and expiry')
  assert.equal(
    cardFillsLine(['number', 'given', 'family', 'month', 'year', 'type']),
    'Fills card number, name, expiry and card type',
  )
  assert.equal(cardFillsLine(['number']), 'Fills card number')
})

test('expired cards are left out of the card', () => {
  const now = new Date(2026, 5, 15)
  const saved = [
    { id: 1, nickname: 'Travel', brand: 'visa', last4: '4242', exp_month: 8, exp_year: 2028 },
    { id: 2, nickname: '', brand: 'mastercard', last4: '4444', exp_month: 5, exp_year: 2026 },
  ]
  assert.deepEqual(cardRows(saved, now), [
    { id: 1, title: 'Travel', detail: 'Visa •••• 4242 · Expires 08/28' },
  ])
})
