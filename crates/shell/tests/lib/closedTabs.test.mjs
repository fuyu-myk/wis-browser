// The palette reaches every closed tab the backend keeps.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { CLOSED_TAB_LIMIT } from '../../src/lib/palette.ts'
import { readSource } from '../support/l3-harness.mjs'

test('the palette asks for as many closed tabs as Recently closed keeps', () => {
  const rust = readSource('src-tauri/src/commands/tab.rs')
  assert.match(rust, new RegExp(`pub const CLOSED_TAB_CAP: u32 = ${CLOSED_TAB_LIMIT};`))
})
