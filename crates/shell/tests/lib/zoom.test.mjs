// The zoom indicator's view of per-site zoom (`features/zoom/zoom.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  ZOOM_LADDER,
  hostOfUrl,
  saneZoomPercent,
  zoomPercentForHost,
} from '../../src/features/zoom/zoom.ts'
import { readSource } from '../support/l3-harness.mjs'

test('a page URL gives its hostname, without port', () => {
  assert.equal(hostOfUrl('https://www.example.com:8443/a?b'), 'www.example.com')
})

test('a URL with no host or no parse gives an empty host', () => {
  assert.equal(hostOfUrl('about:blank'), '')
  assert.equal(hostOfUrl('not a url'), '')
})

test('a site with its own zoom gets it', () => {
  assert.equal(zoomPercentForHost('example.com', { 'example.com': 150 }, 100), 150)
})

test('a site without its own zoom gets the default', () => {
  assert.equal(zoomPercentForHost('other.com', { 'example.com': 150 }, 110), 110)
  assert.equal(zoomPercentForHost('', {}, 90), 90)
})

test('the host is matched ignoring case, spaces and a leading www.', () => {
  assert.equal(zoomPercentForHost('  WWW.Example.COM ', { 'example.com': 80 }, 100), 80)
})

test('a subdomain does not inherit the parent site zoom', () => {
  assert.equal(zoomPercentForHost('docs.example.com', { 'example.com': 80 }, 100), 100)
})

test('the page URL feeds straight into the lookup', () => {
  const host = hostOfUrl('https://www.example.com/watch')
  assert.equal(zoomPercentForHost(host, { 'example.com': 130 }, 100), 130)
})

test('a zoom off the ladder reads as the rung that applies, as in Rust', () => {
  assert.equal(zoomPercentForHost('example.com', { 'example.com': 125 }, 100), 120)
  assert.equal(zoomPercentForHost('other.com', { 'example.com': 150 }, 125), 120)
})

test('a host named like an Object.prototype member gets the default', () => {
  for (const host of ['constructor', 'toString', '__proto__']) {
    assert.equal(zoomPercentForHost(host, {}, 100), 100, host)
  }
})

test('the ladder is the one wis_core::sites::zoom::ZOOM_LADDER climbs', () => {
  const rust = readSource('../core/src/sites/zoom.rs')
  const body = rust.match(/pub const ZOOM_LADDER: \[u16; \d+\] = \[([^\]]*)\]/)
  assert.ok(body, 'ZOOM_LADDER not found')
  const rungs = body[1].split(',').map((rung) => rung.trim()).filter(Boolean).map(Number)
  assert.deepEqual(ZOOM_LADDER, rungs)
})

test('a percentage off the ladder reads as the rung that applies, ties to the smaller', () => {
  assert.equal(saneZoomPercent(125), 120)
  assert.equal(saneZoomPercent(126), 130)
  assert.equal(saneZoomPercent(350), 300)
  assert.equal(saneZoomPercent(351), 400)
})

test('a percentage past either end reads as that end, and a rung as itself', () => {
  assert.equal(saneZoomPercent(0), 20)
  assert.equal(saneZoomPercent(9999), 500)
  for (const rung of ZOOM_LADDER) assert.equal(saneZoomPercent(rung), rung)
})
