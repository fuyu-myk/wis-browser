// Reducing a typed or pasted site to the host a per-site override is keyed by (`features/site/lib/host.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { normalizeHostInput, siteUrl } from '../../src/features/site/lib/host.ts'
import { readSource } from '../support/l3-harness.mjs'

test('blank input gives no host', () => {
  assert.equal(normalizeHostInput(''), '')
  assert.equal(normalizeHostInput('   '), '')
})

test('a pasted URL is reduced to its host', () => {
  assert.equal(normalizeHostInput('https://www.Example.com/path?q=1#top'), 'example.com')
})

test('credentials and port are dropped from a pasted URL', () => {
  assert.equal(normalizeHostInput('https://user:pw@www.example.com:8443/x'), 'example.com')
})

test('host/path keeps only the host', () => {
  assert.equal(normalizeHostInput('example.com/foo/bar'), 'example.com')
})

test('a host/path whose path holds a URL keeps the host', () => {
  assert.equal(normalizeHostInput('www.example.com/redirect?to=https://other.test'), 'example.com')
})

test('surrounding space and case are folded away', () => {
  assert.equal(normalizeHostInput('  WWW.Example.COM  '), 'example.com')
})

test('only a leading www. is stripped', () => {
  assert.equal(normalizeHostInput('wwwx.com'), 'wwwx.com')
  assert.equal(normalizeHostInput('cdn.www.example.com'), 'cdn.www.example.com')
})

test('a subdomain is kept', () => {
  assert.equal(normalizeHostInput('https://news.example.com'), 'news.example.com')
})

test("an unparseable URL is no host, never the host 'https:'", () => {
  assert.equal(normalizeHostInput('https://exa mple.com/x'), '')
})

test('a typed host:port drops the port, as the Rust lookup keys by host alone', () => {
  assert.equal(normalizeHostInput('localhost:3000'), 'localhost')
  assert.equal(normalizeHostInput('www.example.com:8443/path'), 'example.com')
})

test('a page belongs to its own URL, a blob page to the web page that made it', () => {
  assert.equal(siteUrl('https://news.example/a?b')?.href, 'https://news.example/a?b')
  assert.equal(siteUrl('about:blank')?.href, 'about:blank')
  assert.equal(siteUrl('blob:https://www.video.example/0b5f7c1e')?.href, 'https://www.video.example/0b5f7c1e')
})

test('a blob from anywhere but a web page, or no URL at all, has no site', () => {
  assert.equal(siteUrl('blob:null/0b5f7c1e'), null)
  assert.equal(siteUrl('blob:ws://video.example/0b5f7c1e'), null)
  assert.equal(siteUrl('blob:blob:https://video.example/0b5f7c1e'), null)
  assert.equal(siteUrl('not a url'), null)
})

test('the blob rule is the one wis_core::sites::domain::site_url applies', () => {
  const rust = readSource('../core/src/sites/domain.rs')
  assert.ok(rust.includes('Url::parse(url.path())'))
  assert.ok(rust.includes('matches!(inner.scheme(), "http" | "https")'))
})
