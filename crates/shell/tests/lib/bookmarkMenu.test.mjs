// The bookmarks popup and star folder-picker contract (`lib/bookmarkMenu.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import * as bm from '../../src/lib/bookmarkMenu.ts'
import { filesSpelling } from '../support/l3-harness.mjs'

test('the last-used folder is remembered per workspace under a stable key', () => {
  assert.equal(bm.lastFolderKey('ws-1'), 'wis-bm-last-folder-ws-1')
  assert.notEqual(bm.lastFolderKey('ws-1'), bm.lastFolderKey('ws-2'))
})

test('the popup and picker events are distinct and spelled only in this contract', () => {
  const names = [
    bm.BOOKMARKS_OPEN_EVENT,
    bm.BOOKMARK_PAGE_EVENT,
    bm.BOOKMARKS_CLOSED_EVENT,
    bm.BMPICKER_OPEN_EVENT,
    bm.BMPICKER_CLOSE_EVENT,
  ]
  assert.equal(new Set(names).size, names.length)
  for (const name of names) assert.deepEqual(filesSpelling(name), ['lib/bookmarkMenu.ts'], name)
})
