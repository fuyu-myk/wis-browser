// The site panel's placement and protection lookups (`lib/sitePanel.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import {
  PANEL_WIDTH,
  blockedLine,
  isUnprotected,
  panelBounds,
  shownHosts,
  siteOf,
  siteProtection,
} from '../../src/lib/sitePanel.ts'

const viewport = { width: 1200, height: 800 }
const padlock = { x: 300, y: 20, width: 20, height: 24 }
const off = { blocking: false, javascript: true }

describe('panelBounds', () => {
  test('opens below the padlock, its left edge on the padlock’s', () => {
    assert.deepEqual(panelBounds(padlock, 120, viewport), { x: 300, y: 50, width: PANEL_WIDTH, height: 120 })
  })

  test('stays inside the window near its right edge', () => {
    const bounds = panelBounds({ ...padlock, x: 1100 }, 120, viewport)
    assert.equal(bounds.x, 1200 - 8 - PANEL_WIDTH)
  })

  test('opens above the padlock when there is no room below', () => {
    const bounds = panelBounds({ ...padlock, y: 740 }, 120, viewport)
    assert.equal(bounds.y + bounds.height, 734)
  })
})

describe('siteOf', () => {
  test('is the host without www for websites only', () => {
    assert.equal(siteOf('https://www.News.example:8443/a?b'), 'news.example')
    assert.equal(siteOf('http://localhost:3000/'), 'localhost')
    assert.equal(siteOf('about:settings'), null)
    assert.equal(siteOf('tauri://localhost/index.html'), null)
    assert.equal(siteOf('not a url'), null)
  })
})

describe('siteProtection', () => {
  test('is on for sites without an override', () => {
    assert.deepEqual(siteProtection(undefined, undefined, 'a.example'), { blocking: true, javascript: true })
    assert.deepEqual(siteProtection({ 'b.example': off }, {}, 'a.example'), { blocking: true, javascript: true })
  })

  test('reads the site’s saved override', () => {
    assert.deepEqual(siteProtection({ 'a.example': off }, {}, 'a.example'), off)
  })

  test('prefers a switch changed in a private workspace this session', () => {
    const on = { blocking: true, javascript: true }
    assert.deepEqual(siteProtection({ 'a.example': off }, { 'a.example': on }, 'a.example'), on)
  })

  test('counts as off when either switch is', () => {
    assert.equal(isUnprotected({ blocking: true, javascript: true }), false)
    assert.equal(isUnprotected(off), true)
    assert.equal(isUnprotected({ blocking: true, javascript: false }), true)
  })
})

describe('what was blocked', () => {
  test('reads as a count on the page', () => {
    assert.equal(blockedLine(0), 'Nothing blocked on this page')
    assert.equal(blockedLine(1), '1 blocked on this page')
    assert.equal(blockedLine(14), '14 blocked on this page')
  })

  test('lists the most blocked servers and counts the rest', () => {
    const hosts = Array.from({ length: 10 }, (_, n) => [`h${n}.test`, 10 - n])
    assert.deepEqual(shownHosts(hosts, 8), { shown: hosts.slice(0, 8), more: 2 })
    assert.deepEqual(shownHosts(hosts.slice(0, 3), 8), { shown: hosts.slice(0, 3), more: 0 })
  })
})
