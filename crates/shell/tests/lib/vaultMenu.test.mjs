// The URL-bar vault menu's events and the captured-login payload
// (`features/vault/lib/vaultMenu.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import * as vault from '../../src/features/vault/lib/vaultMenu.ts'
import {
  filesSpelling,
  interfaceFields,
  readSource,
  serializedStructFields,
} from '../support/l3-harness.mjs'

test('the captured-login event is the one vault/capture.rs emits', () => {
  const rust = readSource('src-tauri/src/vault/capture.rs')
  assert.ok(rust.includes(`const CAPTURE_EVENT: &str = "${vault.CAPTURE_LOGIN_EVENT}";`))
})

test('the capture payload has exactly the fields vault/capture.rs serialises', () => {
  const ts = interfaceFields(readSource('src/features/vault/lib/vaultMenu.ts'), 'CaptureInfo')
  const rust = serializedStructFields(readSource('src-tauri/src/vault/capture.rs'), 'CaptureInfo')
  assert.deepEqual(ts, rust)
})

test('the menu events are distinct and spelled only in this contract', () => {
  const names = [vault.VAULTMENU_OPEN_EVENT, vault.VAULTMENU_CLOSE_EVENT, vault.CAPTURE_LOGIN_EVENT]
  assert.equal(new Set(names).size, names.length)
  for (const name of names) assert.deepEqual(filesSpelling(name), ['features/vault/lib/vaultMenu.ts'], name)
})
