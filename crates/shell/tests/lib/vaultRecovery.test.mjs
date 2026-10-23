// Settings' recovery key display and the unlock failure message (`features/vault/lib/vaultRecovery.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { shortKey, unlockProblem } from '../../src/features/vault/lib/vaultRecovery.ts'

test('a recovery key is shown short enough to recognise', () => {
  assert.equal(
    shortKey('age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p'),
    'age1ql3z7h…qmcac8p',
  )
  assert.equal(
    shortKey('ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIM5w5pZlTfiaj0O1Iswbm5/lmixpWkEMBZaJuGrkM1Jg me@laptop'),
    'ssh-ed25519 …GrkM1Jg me@laptop',
  )
  assert.equal(shortKey('ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIM5w5pZlTfiaj0O1Iswbm5'), 'ssh-ed25519 …1Iswbm5')
})

test('a failed unlock says what happened and where to fix it', () => {
  assert.equal(
    unlockProblem({ message: 'The key is missing' }),
    'The key is missing. Settings > Passwords can recover your vault.',
  )
})
