// The cookie-notice and annoyance switches (`features/privacy/lib/noticeLists.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { noticeListsPatch } from '../../src/features/privacy/lib/noticeLists.ts'

test('annoyances carry cookie notices, which they contain', () => {
  assert.deepEqual(noticeListsPatch('annoyances', true), { block_annoyances: true, block_cookie_notices: true })
  assert.deepEqual(noticeListsPatch('cookie_notices', false), { block_cookie_notices: false, block_annoyances: false })
})

test('each switch otherwise moves alone', () => {
  assert.deepEqual(noticeListsPatch('annoyances', false), { block_annoyances: false })
  assert.deepEqual(noticeListsPatch('cookie_notices', true), { block_cookie_notices: true })
})
