// The encrypted-DNS provider picker and which provider a stored URL means (`lib/doh.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { DOH_PROVIDERS, dohProviderFor } from '../../src/lib/doh.ts'

test('no stored resolver means Cloudflare', () => {
  assert.equal(dohProviderFor(null), 'cloudflare')
})

test('each offered provider maps back to itself', () => {
  for (const provider of DOH_PROVIDERS) {
    assert.equal(dohProviderFor(provider.url), provider.id, provider.url)
  }
})

test('the offered providers are distinct, over HTTPS, and each has a note', () => {
  assert.deepEqual(
    DOH_PROVIDERS.map((p) => p.id),
    ['cloudflare', 'quad9', 'google'],
  )
  for (const provider of DOH_PROVIDERS) {
    assert.equal(new URL(provider.url).protocol, 'https:', provider.id)
    assert.ok(provider.label && provider.note, provider.id)
  }
})

test('Quad9 is recognised by its host or either anycast address', () => {
  for (const url of ['https://dns.quad9.net/other', 'https://9.9.9.9/dns-query', 'https://149.112.112.112/dns-query']) {
    assert.equal(dohProviderFor(url), 'quad9', url)
  }
})

test('Google is recognised by its host or either anycast address', () => {
  for (const url of ['https://dns.google/resolve', 'https://8.8.8.8/dns-query', 'https://8.8.4.4/dns-query']) {
    assert.equal(dohProviderFor(url), 'google', url)
  }
})

test('the host is matched case-insensitively', () => {
  assert.equal(dohProviderFor('https://DNS.Quad9.NET/dns-query'), 'quad9')
})

test('an unrecognised or unparseable URL resolves through Cloudflare', () => {
  assert.equal(dohProviderFor('https://doh.example.net/dns-query'), 'cloudflare')
  assert.equal(dohProviderFor('https://dns.google.evil.test/dns-query'), 'cloudflare')
  assert.equal(dohProviderFor('not a url'), 'cloudflare')
  assert.equal(dohProviderFor(''), 'cloudflare')
})
