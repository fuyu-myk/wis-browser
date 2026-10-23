// The contract between the page-menu opener and its overlay surface
// (`features/page-tools/pageMenu.ts`).

import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { test } from 'node:test'

import * as menu from '../../src/features/page-tools/pageMenu.ts'
import {
  filesSpelling,
  interfaceFields,
  readSource,
  serializedStructFields,
} from '../support/l3-harness.mjs'

test('the menu events are distinct and spelled only in this contract', () => {
  const names = [
    menu.PAGE_MENU_OPEN_EVENT,
    menu.PAGE_MENU_READY_EVENT,
    menu.PAGE_MENU_DONE_EVENT,
    menu.PAGE_MENU_ACK_EVENT,
  ]
  assert.equal(new Set(names).size, names.length)
  for (const name of names) assert.deepEqual(filesSpelling(name), ['features/page-tools/pageMenu.ts'], name)
})

test('the page report has exactly the fields pages/context_menu.rs serialises', () => {
  const ts = interfaceFields(readSource('src/features/page-tools/pageMenu.ts'), 'PageMenuReport')
  const rust = serializedStructFields(readSource('src-tauri/src/pages/context_menu.rs'), 'PageMenuEvent')
  assert.deepEqual(ts, rust)
})

test('every menu target attribute the chrome writes is one the opener reads', () => {
  const src = new URL('../../src/', import.meta.url)
  const written = new Set()
  for (const file of readdirSync(src, { recursive: true })) {
    if (!file.endsWith('.tsx')) continue
    const text = readFileSync(new URL(file, src), 'utf8')
    for (const [, attr] of text.matchAll(/\s(data-menu-[a-z-]+)=/g)) written.add(attr)
  }
  assert.deepEqual([...written].sort(), [menu.MENU_LABEL_ATTR, menu.MENU_URL_ATTR].sort())
})
