// Accent colours handed to new workspaces and folders (`lib/accents.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { ACCENT_ROTATION, FOLDER_ACCENTS, nextAccent } from '../../src/lib/accents.ts'

test('the workspace and folder cycles use the same accents, each once', () => {
  assert.equal(new Set(ACCENT_ROTATION).size, ACCENT_ROTATION.length)
  assert.deepEqual([...FOLDER_ACCENTS].sort(), [...ACCENT_ROTATION].sort())
})

test('a new workspace takes the first accent no workspace uses yet', () => {
  assert.equal(nextAccent([]), 'primary')
  assert.equal(nextAccent(['primary', 'rose']), 'mint')
  assert.equal(nextAccent(['mint', 'primary', 'sun', 'rose']), 'lila')
})

test('once every accent is taken the cycle continues by workspace count', () => {
  assert.equal(nextAccent([...ACCENT_ROTATION]), 'primary')
  assert.equal(nextAccent([...ACCENT_ROTATION, 'ink']), 'mint')
  assert.equal(nextAccent([...ACCENT_ROTATION, ...ACCENT_ROTATION.slice(0, 3)]), 'sun')
})
