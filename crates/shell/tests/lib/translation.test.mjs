// The address bar's translate button (`features/translation/translation.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { translateButton } from '../../src/features/translation/translation.ts'

const tab = (fields) => ({ translated: false, translatable_from: null, ...fields })

test('a page in a language the reader does not read offers to translate from it', () => {
  assert.deepEqual(translateButton(tab({ translatable_from: 'French' })), {
    label: 'Translate from French',
    pressed: false,
  })
})

test('a translated page offers its original, whatever its language', () => {
  assert.deepEqual(translateButton(tab({ translated: true })), { label: 'Show original', pressed: true })
  assert.deepEqual(translateButton(tab({ translated: true, translatable_from: 'French' })), {
    label: 'Show original',
    pressed: true,
  })
})

test('a page in a language the reader reads, or no page, has no button', () => {
  assert.equal(translateButton(tab({})), null)
  assert.equal(translateButton(null), null)
})
