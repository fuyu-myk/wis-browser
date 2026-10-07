// Whether overlay menus render glass (`lib/glass.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { menusGlass } from '../../src/lib/glass.ts'

const settings = (glass_enabled, menus) => ({ glass_enabled, design: { glass_scopes: { menus } } })

test('menus are glass only with the master toggle and the menus scope both on', () => {
  assert.equal(menusGlass(settings(true, true)), true)
  assert.equal(menusGlass(settings(false, true)), false)
  assert.equal(menusGlass(settings(true, false)), false)
  assert.equal(menusGlass(settings(false, false)), false)
})

test('menus are glass while settings are still loading', () => {
  assert.equal(menusGlass(undefined), true)
})
