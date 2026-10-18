// Where the extensions menu opens (`lib/extensionsMenu.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { MENU_WIDTH, menuBounds, menuHeight } from '../../src/lib/extensionsMenu.ts'

const viewport = { width: 1200, height: 800 }
const button = { x: 900, y: 20, width: 24, height: 24 }

describe('menuBounds', () => {
  test('opens below the button, its right edge on the button’s', () => {
    const bounds = menuBounds(button, 2, viewport)
    assert.equal(bounds.x, 924 - MENU_WIDTH)
    assert.equal(bounds.y, 50)
    assert.equal(bounds.width, MENU_WIDTH)
    assert.equal(bounds.height, menuHeight(2))
  })

  test('stays inside the window near its left edge', () => {
    const bounds = menuBounds({ x: 10, y: 20, width: 24, height: 24 }, 1, viewport)
    assert.equal(bounds.x, 8)
  })

  test('opens above the button when there is no room below', () => {
    const low = { x: 900, y: 760, width: 24, height: 24 }
    const bounds = menuBounds(low, 3, viewport)
    assert.equal(bounds.y + bounds.height, 754)
  })

  test('grows with its rows and keeps room for one when empty', () => {
    assert.ok(menuHeight(3) > menuHeight(1))
    assert.equal(menuHeight(0), menuHeight(1))
  })
})
