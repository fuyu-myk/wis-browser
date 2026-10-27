// Which Settings section the nav scrolls to while the vault is locked
// (`features/vault/lib/vaultSections.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { navTarget, VAULT_SECTION } from '../../src/features/vault/lib/vaultSections.ts'

test('a locked vault sends its sections to the vault', () => {
  for (const id of ['passwords', 'addresses', 'cards']) {
    assert.equal(navTarget(id, false), VAULT_SECTION)
    assert.equal(navTarget(id, true), id)
  }
})

test('other sections are never redirected', () => {
  for (const id of ['appearance', 'backup', VAULT_SECTION]) {
    assert.equal(navTarget(id, false), id)
    assert.equal(navTarget(id, true), id)
  }
})
