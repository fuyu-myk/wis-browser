// The zoom indicator's view of per-site zoom (`lib/zoom.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { hostOfUrl, zoomPercentForHost } from '../../src/lib/zoom.ts'

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
  assert.equal(zoomPercentForHost(host, { 'example.com': 125 }, 100), 125)
})

test('a host named like an Object.prototype member gets the default', () => {
  for (const host of ['constructor', 'toString', '__proto__']) {
    assert.equal(zoomPercentForHost(host, {}, 100), 100, host)
  }
})
