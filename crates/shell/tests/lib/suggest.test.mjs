// Which suggestions a suggest field lists (`lib/suggest.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { matchSuggestions } from '../../src/lib/suggest.ts'

const ITEMS = [
  { value: 'http://localhost:11434/v1', label: 'Ollama' },
  { value: 'http://localhost:1234/v1', label: 'LM Studio' },
  { value: 'https://openrouter.ai/api/v1', label: 'OpenRouter' },
]

test('everything is listed until the user types', () => {
  assert.deepEqual(matchSuggestions(ITEMS, 'https://openrouter.ai/api/v1', false), ITEMS)
})

test('typed text keeps the items whose value or label contains it, ignoring case', () => {
  assert.deepEqual(
    matchSuggestions(ITEMS, 'studio', true).map((item) => item.label),
    ['LM Studio'],
  )
  assert.deepEqual(
    matchSuggestions(ITEMS, 'LOCALHOST', true).map((item) => item.label),
    ['Ollama', 'LM Studio'],
  )
})

test('blank typed text lists everything', () => {
  assert.deepEqual(matchSuggestions(ITEMS, '   ', true), ITEMS)
})

test('an exact match alone is not offered back', () => {
  assert.deepEqual(matchSuggestions(ITEMS, 'http://localhost:1234/v1', true), [])
})
