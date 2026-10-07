// The event that sends a selection from the page menu to the assistant
// (`lib/aiAsk.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { AI_ASK_EVENT } from '../../src/lib/aiAsk.ts'
import { filesSpelling, readSource } from '../support/l3-harness.mjs'

test('the ask event is spelled only in this contract', () => {
  assert.deepEqual(filesSpelling(AI_ASK_EVENT), ['lib/aiAsk.ts'])
})

test('the page menu sends it and the chrome listens for it through the contract', () => {
  const contract = /import \{[^}]*\bAI_ASK_EVENT\b[^}]*\} from '@\/lib\/aiAsk'/
  assert.match(readSource('src/overlay/surfaces/PageMenuSurface.tsx'), contract)
  assert.match(readSource('src/hooks/useAiAsk.ts'), contract)
})
