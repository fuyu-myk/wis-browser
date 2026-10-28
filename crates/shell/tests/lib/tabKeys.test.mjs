// What keys do on a focused tab (`features/tabs/lib/tabKeys.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  crossedGroup,
  moveDelta,
  movedAcross,
  movedOrder,
  movedTo,
  neighbourAfterClose,
  tabKeyAction,
  withGroupOrder,
} from '../../src/features/tabs/lib/tabKeys.ts'

const press = (key, mods = {}) => ({ key, shiftKey: false, altKey: false, ctrlKey: false, metaKey: false, ...mods })

test('Enter and Space switch to the tab', () => {
  assert.equal(tabKeyAction(press('Enter')), 'activate')
  assert.equal(tabKeyAction(press(' ')), 'activate')
})

test('Delete and Backspace close it', () => {
  assert.equal(tabKeyAction(press('Delete')), 'close')
  assert.equal(tabKeyAction(press('Backspace')), 'close')
})

test('Shift+F10 and the menu key open its menu', () => {
  assert.equal(tabKeyAction(press('F10', { shiftKey: true })), 'menu')
  assert.equal(tabKeyAction(press('ContextMenu')), 'menu')
  assert.equal(tabKeyAction(press('F10')), null)
})

test('modified presses are left to the shortcuts', () => {
  assert.equal(tabKeyAction(press('Backspace', { metaKey: true })), null)
  assert.equal(tabKeyAction(press('Enter', { altKey: true })), null)
  assert.equal(tabKeyAction(press('a')), null)
})

test('closing a tab hands focus to the next one, or the previous at the end', () => {
  assert.equal(neighbourAfterClose(['a', 'b', 'c'], 'b'), 'c')
  assert.equal(neighbourAfterClose(['a', 'b', 'c'], 'c'), 'b')
  assert.equal(neighbourAfterClose(['a'], 'a'), null)
  assert.equal(neighbourAfterClose(['a', 'b'], 'x'), null)
})

test('Alt with the strip\'s arrows moves the focused item, other arrows do not', () => {
  assert.equal(moveDelta(press('ArrowUp', { altKey: true }), 'vertical'), -1)
  assert.equal(moveDelta(press('ArrowDown', { altKey: true }), 'vertical'), 1)
  assert.equal(moveDelta(press('ArrowLeft', { altKey: true }), 'horizontal'), -1)
  assert.equal(moveDelta(press('ArrowRight', { altKey: true }), 'horizontal'), 1)
  assert.equal(moveDelta(press('ArrowLeft', { altKey: true }), 'vertical'), null)
  assert.equal(moveDelta(press('ArrowDown'), 'vertical'), null)
  assert.equal(moveDelta(press('ArrowDown', { altKey: true, metaKey: true }), 'vertical'), null)
})

test('a keyboard move shifts the item one place and stops at the ends', () => {
  assert.deepEqual(movedOrder(['a', 'b', 'c'], 'b', -1), ['b', 'a', 'c'])
  assert.deepEqual(movedOrder(['a', 'b', 'c'], 'b', 1), ['a', 'c', 'b'])
  assert.equal(movedOrder(['a', 'b', 'c'], 'a', -1), null)
  assert.equal(movedOrder(['a', 'b', 'c'], 'c', 1), null)
  assert.equal(movedOrder(['a', 'b'], 'x', 1), null)
})

test('a group\'s new order is laid back into the full order in place', () => {
  assert.deepEqual(withGroupOrder(['a', 'x', 'b', 'y', 'c'], ['c', 'a', 'b']), ['c', 'x', 'a', 'y', 'b'])
  assert.equal(movedTo(2, 5), 'Moved to 2 of 5')
})

// Open folders in order, then the loose tabs.
const GROUPS = [
  { folder: 'work', size: 2 },
  { folder: 'read', size: 3 },
  { folder: null, size: 4 },
]

test('past the end of a folder a tab joins the start of the next group', () => {
  assert.deepEqual(crossedGroup(GROUPS, 'work', 1), { folder: 'read', index: 0 })
  assert.deepEqual(crossedGroup(GROUPS, 'read', 1), { folder: null, index: 0 })
})

test('past the start of a group a tab joins the end of the one before', () => {
  assert.deepEqual(crossedGroup(GROUPS, null, -1), { folder: 'read', index: 3 })
  assert.deepEqual(crossedGroup(GROUPS, 'read', -1), { folder: 'work', index: 2 })
})

test('a tab stays put at either end of the strip or outside the groups', () => {
  assert.equal(crossedGroup(GROUPS, 'work', -1), null)
  assert.equal(crossedGroup(GROUPS, null, 1), null)
  assert.equal(crossedGroup(GROUPS, 'closed', 1), null)
})

test('an empty folder takes the tab as its only member', () => {
  const groups = [{ folder: 'empty', size: 0 }, { folder: null, size: 1 }]
  assert.deepEqual(crossedGroup(groups, null, -1), { folder: 'empty', index: 0 })
})

test('a move across groups names where the tab went', () => {
  assert.equal(movedAcross('Reading', null, 1, 3), 'Moved into Reading, 1 of 3')
  assert.equal(movedAcross(null, 'Work', 1, 4), 'Moved out of Work, 1 of 4')
})
