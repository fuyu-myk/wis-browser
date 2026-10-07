// React Query keys for the per-workspace bookmark stores (`lib/bookmarkKeys.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { bookmarkFoldersKey, bookmarksKey } from '../../src/lib/bookmarkKeys.ts'
import { readSource } from '../support/l3-harness.mjs'

test('each store is keyed by its name and the workspace', () => {
  assert.deepEqual(bookmarksKey('ws-1'), ['bookmarks', 'ws-1'])
  assert.deepEqual(bookmarkFoldersKey('ws-1'), ['bookmark-folders', 'ws-1'])
})

test('workspaces and stores never share a key', () => {
  const keys = [bookmarksKey('a'), bookmarksKey('b'), bookmarkFoldersKey('a'), bookmarkFoldersKey('b')]
  assert.equal(new Set(keys.map((k) => JSON.stringify(k))).size, keys.length)
})

test('the cross-webview sync invalidates both stores by their leading key', () => {
  const sync = readSource('src/hooks/useBookmarkSync.ts')
  assert.ok(sync.includes(`k === '${bookmarksKey('x')[0]}'`))
  assert.ok(sync.includes(`k === '${bookmarkFoldersKey('x')[0]}'`))
})
