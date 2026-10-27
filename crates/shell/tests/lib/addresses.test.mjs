// How Settings shows a saved address and its country (`features/vault/lib/addresses.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { addressFields, addressMatches, addressSummary, countryOptions, emptyAddress } from '../../src/features/vault/lib/addresses.ts'

test('an address is shown by its name, or what it has instead, and where it is', () => {
  const home = {
    ...emptyAddress(),
    given_name: 'Ada',
    family_name: 'Lovelace',
    street_address: '12 St James Square\nFlat 3',
    city: 'London',
    postal_code: 'SW1Y 4JH',
    country: 'GB',
  }
  assert.deepEqual(addressSummary(home, 'en'), {
    title: 'Ada Lovelace',
    detail: '12 St James Square, London, SW1Y 4JH, United Kingdom',
  })
  assert.deepEqual(addressSummary({ ...emptyAddress(), organization: 'Analytical Ltd', city: 'London' }, 'en'), {
    title: 'Analytical Ltd',
    detail: 'London',
  })
  assert.deepEqual(addressSummary({ ...emptyAddress(), street_address: '1 Main St\nUnit 2' }, 'en'), {
    title: '1 Main St',
    detail: '',
  })
})

test('countries are listed by name for the reader', () => {
  const options = countryOptions('en')
  assert.ok(options.length > 240)
  assert.deepEqual(options.find((option) => option.code === 'GB'), { code: 'GB', name: 'United Kingdom' })
  const names = options.map((option) => option.name)
  assert.deepEqual(names, [...names].sort(new Intl.Collator('en').compare))
})

test('a search finds an address by any of its fields or its country, ignoring case', () => {
  const home = {
    ...emptyAddress(),
    given_name: 'Ada',
    family_name: 'Lovelace',
    street_address: '12 St James Square\nFlat 3',
    city: 'London',
    postal_code: 'SW1Y 4JH',
    country: 'GB',
    email: 'ada@example.org',
  }
  for (const query of ['', '  ', 'ada', 'LOVELACE', 'flat 3', 'sw1y', 'united king', 'example.org']) {
    assert.equal(addressMatches(home, query, 'en'), true, query)
  }
  for (const query of ['paris', 'babbage', 'france']) {
    assert.equal(addressMatches(home, query, 'en'), false, query)
  }
})

test('an open address lists only the fields it has, street lines kept', () => {
  const address = {
    ...emptyAddress(),
    given_name: 'Ada',
    family_name: 'Lovelace',
    street_address: '12 St James Sq\nFlat 3',
    city: 'London',
    postal_code: 'SW1Y 4JH',
    country: 'GB',
    email: 'ada@example.com',
  }
  assert.deepEqual(addressFields(address, 'en'), [
    { label: 'Name', value: 'Ada Lovelace' },
    { label: 'Street', value: '12 St James Sq\nFlat 3' },
    { label: 'City', value: 'London' },
    { label: 'Postal code', value: 'SW1Y 4JH' },
    { label: 'Country', value: 'United Kingdom' },
    { label: 'Email', value: 'ada@example.com' },
  ])
  assert.deepEqual(addressFields(emptyAddress(), 'en'), [])
})
