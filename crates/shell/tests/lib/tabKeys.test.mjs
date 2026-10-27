// What keys do on a focused tab (`features/tabs/lib/tabKeys.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { moveDelta, movedOrder, movedTo, neighbourAfterClose, tabKeyAction, withGroupOrder } from '../../src/features/tabs/lib/tabKeys.ts'

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
