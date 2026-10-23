// The site panel's placement and protection lookups (`features/site/lib/sitePanel.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import {
  PANEL_WIDTH,
  applicableProtection,
  blockedLine,
  fingerprintingApplies,
  isUnprotected,
  panelBounds,
  protectionLabel,
  shownHosts,
  siteOf,
  siteProtection,
} from '../../src/features/site/lib/sitePanel.ts'

const viewport = { width: 1200, height: 800 }
const padlock = { x: 300, y: 20, width: 20, height: 24 }
const on = { blocking: true, javascript: true, fingerprinting: true }
const off = { ...on, blocking: false }

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
    assert.deepEqual(siteProtection(undefined, undefined, 'a.example'), on)
    assert.deepEqual(siteProtection({ 'b.example': off }, {}, 'a.example'), on)
  })

  test('reads the site’s saved override', () => {
    assert.deepEqual(siteProtection({ 'a.example': off }, {}, 'a.example'), off)
  })

  test('prefers a switch changed in a private workspace this session', () => {
    assert.deepEqual(siteProtection({ 'a.example': off }, { 'a.example': on }, 'a.example'), on)
  })

  test('counts as off when either switch is', () => {
    assert.equal(isUnprotected(on), false)
    assert.equal(isUnprotected(off), true)
    assert.equal(isUnprotected({ ...on, javascript: false }), true)
    assert.equal(isUnprotected({ ...on, fingerprinting: false }), true)
  })
})

describe('applicableProtection', () => {
  test('leaves fingerprinting protection out where it does not apply', () => {
    const exposed = { ...on, fingerprinting: false }
    assert.deepEqual(applicableProtection(exposed, false), on)
    assert.deepEqual(applicableProtection(exposed, true), exposed)
    assert.deepEqual(applicableProtection(off, false), off)
  })

  test('fingerprinting protection never applies in Permissive workspaces', () => {
    assert.equal(fingerprintingApplies('permissive'), false)
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

describe('protectionLabel', () => {
  test('names what is switched off', () => {
    assert.equal(protectionLabel(off), 'Blocking off')
    assert.equal(protectionLabel({ ...on, javascript: false }), 'JavaScript off')
    assert.equal(protectionLabel({ ...on, fingerprinting: false }), 'Fingerprinting protection off')
    assert.equal(protectionLabel({ ...off, javascript: false }), 'Blocking and JavaScript off')
    assert.equal(protectionLabel({ ...off, fingerprinting: false }), 'Blocking and fingerprinting protection off')
    assert.equal(
      protectionLabel({ blocking: false, javascript: false, fingerprinting: false }),
      'Blocking, JavaScript and fingerprinting protection off',
    )
    assert.equal(protectionLabel(on), 'Protected')
  })
})
