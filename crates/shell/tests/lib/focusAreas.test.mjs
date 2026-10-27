// F6 / Shift+F6 cycle between the address bar, the tabs and the page
// (`features/keyboard/lib/focusAreas.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { nextArea } from '../../src/features/keyboard/lib/focusAreas.ts'

test('F6 goes address bar, tabs, page, and round again', () => {
  assert.equal(nextArea('url', false), 'tabs')
  assert.equal(nextArea('tabs', false), 'page')
  assert.equal(nextArea('page', false), 'url')
})

test('Shift+F6 goes the other way', () => {
  assert.equal(nextArea('url', true), 'page')
  assert.equal(nextArea('tabs', true), 'url')
  assert.equal(nextArea('page', true), 'tabs')
})

test('from anywhere else in the chrome, F6 starts at the address bar and Shift+F6 at the page', () => {
  assert.equal(nextArea('other', false), 'url')
  assert.equal(nextArea('other', true), 'page')
})
