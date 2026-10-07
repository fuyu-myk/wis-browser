// The window's tab history (`lib/tabMru.ts`): switch-to-last-tab, the tab a
// close returns to, and the order a closed split pane is refilled from.

import assert from 'node:assert/strict'
import { beforeEach, test } from 'node:test'

import {
  closeReturnTarget,
  noteActiveTab,
  noteTabClosed,
  previousTab,
  pruneTabMru,
  recentTabs,
} from '../src/lib/tabMru.ts'

beforeEach(() => pruneTabMru(() => false))

test('most recent first, each tab once', () => {
  for (const id of ['a', 'b', 'c', 'a']) noteActiveTab(id, 'ws')
  assert.deepEqual(recentTabs(), ['a', 'c', 'b'])
  assert.equal(previousTab(), 'c')
})

test('the same tab again changes nothing', () => {
  noteActiveTab('a', 'ws')
  noteActiveTab('b', 'ws')
  noteActiveTab('b', 'ws')
  assert.deepEqual(recentTabs(), ['b', 'a'])
})

test('closing the active tab returns to the one before it, in the same workspace only', () => {
  noteActiveTab('a', 'one')
  noteActiveTab('b', 'one')
  assert.equal(closeReturnTarget('b'), 'a')
  assert.equal(closeReturnTarget('a'), null, 'only the active tab has a return target')

  noteActiveTab('c', 'two')
  assert.equal(closeReturnTarget('c'), null, 'no return across workspaces')
})

test('a closed tab leaves the history', () => {
  for (const id of ['a', 'b', 'c']) noteActiveTab(id, 'ws')
  noteTabClosed('b')
  assert.deepEqual(recentTabs(), ['c', 'a'])
})

test('the history is bounded', () => {
  for (let i = 0; i < 40; i++) noteActiveTab(`t${i}`, 'ws')
  assert.equal(recentTabs().length, 32)
  assert.equal(recentTabs()[0], 't39')
})
