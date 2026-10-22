// The rule for a new vault password (`lib/vaultPassword.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { newPasswordProblem } from '../../src/lib/vaultPassword.ts'

test('a new vault password needs twelve characters and a matching repeat', () => {
  assert.equal(newPasswordProblem('', ''), 'Use at least 12 characters')
  assert.equal(newPasswordProblem('elevenchars', 'elevenchars'), 'Use at least 12 characters')
  assert.equal(newPasswordProblem('twelve chars', 'twelve charz'), "The passwords don't match")
  assert.equal(newPasswordProblem('twelve chars', 'twelve chars'), null)
  assert.equal(newPasswordProblem('пароль123456', 'пароль123456'), null)
  assert.equal(newPasswordProblem('😀'.repeat(12), '😀'.repeat(12)), null)
})
