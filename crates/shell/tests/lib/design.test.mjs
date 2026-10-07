// Sidebar width bounds (`lib/design.ts`), which must match core's clamp.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

import {
  SIDEBAR_WIDTH_DEFAULT,
  SIDEBAR_WIDTH_MAX,
  SIDEBAR_WIDTH_MIN,
} from '../../src/lib/design.ts'

test('the sidebar width bounds mirror wis_core::design', () => {
  const core = readFileSync(new URL('../../../core/src/design.rs', import.meta.url), 'utf8')
  const constant = (name) => Number(core.match(new RegExp(`const ${name}: u32 = (\\d+);`))[1])
  assert.equal(SIDEBAR_WIDTH_MIN, constant('SIDEBAR_WIDTH_MIN'))
  assert.equal(SIDEBAR_WIDTH_MAX, constant('SIDEBAR_WIDTH_MAX'))
  assert.equal(SIDEBAR_WIDTH_DEFAULT, constant('SIDEBAR_WIDTH_DEFAULT'))
})

test('the default width lies within the bounds', () => {
  assert.ok(SIDEBAR_WIDTH_MIN <= SIDEBAR_WIDTH_DEFAULT && SIDEBAR_WIDTH_DEFAULT <= SIDEBAR_WIDTH_MAX)
})
