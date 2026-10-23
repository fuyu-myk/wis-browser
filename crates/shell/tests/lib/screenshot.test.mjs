// The screenshot toast and region-selection event names (`features/screenshot/screenshot.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import * as shot from '../../src/features/screenshot/screenshot.ts'
import { filesSpelling } from '../support/l3-harness.mjs'

const NAMES = [
  shot.SCREENSHOT_CAPTURED_EVENT,
  shot.SCREENSHOT_TOAST_SHOWN_EVENT,
  shot.SCREENSHOT_TOAST_CLOSED_EVENT,
  shot.SCREENSHOT_TOAST_READY_EVENT,
  shot.SCREENSHOT_REGION_OPEN_EVENT,
  shot.SCREENSHOT_REGION_BEGIN_EVENT,
  shot.SCREENSHOT_REGION_READY_EVENT,
  shot.SCREENSHOT_REGION_RESULT_EVENT,
]

test('every screenshot event name is distinct', () => {
  assert.equal(new Set(NAMES).size, NAMES.length)
})

test('every screenshot event name is spelled only in this contract', () => {
  for (const name of NAMES) assert.deepEqual(filesSpelling(name), ['features/screenshot/screenshot.ts'], name)
})
