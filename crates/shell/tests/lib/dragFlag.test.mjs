// When a drag's "this was a drag" flag drops (`features/tabs/lib/dragFlag.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { clearAfterClick } from '../../src/features/tabs/lib/dragFlag.ts'

const nextTurn = () => new Promise((resolve) => setTimeout(resolve, 0))

test("the release's own click still sees the flag", () => {
  const flag = { current: true }
  clearAfterClick(flag)
  assert.equal(flag.current, true)
})

test('a drag released elsewhere does not keep the flag for the next click', async () => {
  const flag = { current: true }
  clearAfterClick(flag)
  await nextTurn()
  assert.equal(flag.current, false)
})
