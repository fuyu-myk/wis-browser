// What keys do on a focused tab (`features/tabs/lib/tabKeys.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { neighbourAfterClose, tabKeyAction } from '../../src/features/tabs/lib/tabKeys.ts'

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
