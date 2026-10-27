// Which Settings section the nav scrolls to while the vault is locked
// (`features/vault/lib/vaultSections.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { growsIn, navTarget, noteUnlocked, revealBlock, VAULT_SECTION } from '../../src/features/vault/lib/vaultSections.ts'

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

test('an opened item scrolls only as far as it must, or to its top when taller than the view', () => {
  assert.equal(revealBlock(300, 600), 'nearest')
  assert.equal(revealBlock(600, 600), 'nearest')
  assert.equal(revealBlock(601, 600), 'start')
})

test('the vault sections grow in only just after an unlock on this page', () => {
  assert.equal(growsIn(1000), false, 'never unlocked here: already open, no animation')
  noteUnlocked(1000)
  assert.equal(growsIn(1200), true)
  assert.equal(growsIn(3000), false)
})
