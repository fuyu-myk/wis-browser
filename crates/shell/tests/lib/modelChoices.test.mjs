// The rows a model picker offers (`features/ai/lib/modelChoices.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { modelChoices, SHOWN_MODELS } from '../../src/features/ai/lib/modelChoices.ts'

const models = [
  { id: 'claude-opus-5-5', name: 'Claude Opus 5.5' },
  { id: 'gpt-x', name: 'gpt-x' },
]

describe('modelChoices', () => {
  test('lists every model for an empty query, hinting ids that differ from names', () => {
    assert.deepEqual(modelChoices(models, '', true), [
      { id: 'claude-opus-5-5', label: 'Claude Opus 5.5', hint: 'claude-opus-5-5' },
      { id: 'gpt-x', label: 'gpt-x' },
    ])
  })

  test('matches the query against ids and names, ignoring case', () => {
    assert.deepEqual(
      modelChoices(models, 'OPUS', true).map((c) => c.id),
      ['claude-opus-5-5', 'OPUS'],
    )
    assert.deepEqual(
      modelChoices(models, '5.5', true).map((c) => c.id),
      ['claude-opus-5-5', '5.5'],
    )
  })

  test('offers the typed id once the list has settled without it', () => {
    assert.deepEqual(modelChoices(models, '  my-model ', true), [{ id: 'my-model', label: 'Use “my-model”' }])
    assert.deepEqual(modelChoices(models, 'my-model', false), [])
    assert.deepEqual(modelChoices(null, 'my-model', true), [{ id: 'my-model', label: 'Use “my-model”' }])
  })

  test('does not repeat a typed id the list already has', () => {
    assert.deepEqual(
      modelChoices(models, 'gpt-x', true).map((c) => c.id),
      ['gpt-x'],
    )
  })

  test('shows at most SHOWN_MODELS listed models', () => {
    const many = Array.from({ length: SHOWN_MODELS + 5 }, (_, i) => ({ id: `m${i}`, name: `m${i}` }))
    assert.equal(modelChoices(many, '', true).length, SHOWN_MODELS)
  })
})
