// Base URL suggestions for an OpenAI-compatible provider (`features/ai/lib/aiEndpoints.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { BASE_URL_SUGGESTIONS } from '../../src/features/ai/lib/aiEndpoints.ts'

test('every suggestion is a named http(s) endpoint ending in its API version', () => {
  assert.ok(BASE_URL_SUGGESTIONS.length > 0)
  for (const { url, label } of BASE_URL_SUGGESTIONS) {
    const parsed = new URL(url)
    assert.match(parsed.protocol, /^https?:$/, url)
    assert.match(parsed.pathname, /\/v1$/, url)
    assert.ok(label.trim(), url)
  }
})

test('local servers are plain http on loopback, hosted ones https', () => {
  for (const { url } of BASE_URL_SUGGESTIONS) {
    const { protocol, hostname } = new URL(url)
    assert.equal(protocol === 'http:', hostname === 'localhost', url)
  }
})
