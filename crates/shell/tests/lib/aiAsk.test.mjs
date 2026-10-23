// The event that sends a selection from the page menu to the assistant
// (`features/ai/lib/aiAsk.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { AI_ASK_EVENT } from '../../src/features/ai/lib/aiAsk.ts'
import { filesSpelling, readSource } from '../support/l3-harness.mjs'

test('the ask event is spelled only in this contract', () => {
  assert.deepEqual(filesSpelling(AI_ASK_EVENT), ['features/ai/lib/aiAsk.ts'])
})

test('the page menu sends it and the chrome listens for it through the contract', () => {
  const contract = /import \{[^}]*\bAI_ASK_EVENT\b[^}]*\} from '@\/features\/ai\/lib\/aiAsk'/
  assert.match(readSource('src/features/page-tools/PageMenuSurface.tsx'), contract)
  assert.match(readSource('src/features/ai/hooks/useAiAsk.ts'), contract)
})
