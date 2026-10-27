// How Settings shows a saved card (`features/vault/lib/cards.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { cardFields, cardSummary, emptyCard, expiryYears } from '../../src/features/vault/lib/cards.ts'

const travel = { id: 1, nickname: 'Travel', brand: 'visa', last4: '4242', exp_month: 8, exp_year: 2028 }
const june2026 = new Date(2026, 5, 15)

test('a card is shown by its nickname, or its network and last digits', () => {
  assert.deepEqual(cardSummary(travel, june2026), {
    title: 'Travel',
    detail: 'Visa •••• 4242 · Expires 08/28',
    expired: false,
  })
  assert.deepEqual(cardSummary({ ...travel, nickname: '', brand: 'other' }, june2026), {
    title: 'Card •••• 4242',
    detail: 'Expires 08/28',
    expired: false,
  })
})

test('a card is expired only after its expiry month ends', () => {
  assert.equal(cardSummary({ ...travel, exp_month: 6, exp_year: 2026 }, june2026).expired, false)
  const lapsed = cardSummary({ ...travel, exp_month: 5, exp_year: 2026 }, june2026)
  assert.equal(lapsed.expired, true)
  assert.equal(lapsed.detail, 'Visa •••• 4242 · Expired 05/26')
})

test('a new card starts blank and the expiry years run from this one, keeping a saved past year', () => {
  assert.deepEqual(emptyCard(june2026), { number: '', name: '', exp_month: 6, exp_year: 2026, nickname: '' })
  const years = expiryYears(june2026, 2026)
  assert.equal(years[0], 2026)
  assert.equal(years.length, 16)
  assert.deepEqual(expiryYears(june2026, 2024).slice(0, 2), [2024, 2026])
})

test('an open card lists what is kept unsealed, never the full number', () => {
  assert.deepEqual(cardFields(travel, june2026), [
    { label: 'Network', value: 'Visa' },
    { label: 'Number', value: '•••• 4242' },
    { label: 'Expires', value: '08/28' },
    { label: 'Nickname', value: 'Travel' },
  ])
  assert.deepEqual(cardFields({ ...travel, nickname: '', exp_month: 5, exp_year: 2026 }, june2026), [
    { label: 'Network', value: 'Visa' },
    { label: 'Number', value: '•••• 4242' },
    { label: 'Expired', value: '05/26' },
  ])
})
