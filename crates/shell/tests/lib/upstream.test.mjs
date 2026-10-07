// Inline checks for a workspace's typed upstream proxy (`lib/upstream.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import {
  DEFAULT_SOCKS5_PORT,
  proxyHostProblem,
  proxyPortProblem,
  upstreamMode,
} from '../../src/lib/upstream.ts'

const SHAPE = 'Enter a host and port only, with no scheme, credentials or path.'
const INVALID = 'That host is not valid.'

describe('routing mode', () => {
  test('no upstream is a direct connection', () => {
    assert.equal(upstreamMode(null), 'off')
  })

  test('the built-in Tor client and a SOCKS5 proxy are told apart', () => {
    assert.equal(upstreamMode({ kind: 'tor' }), 'tor')
    assert.equal(upstreamMode({ kind: 'socks5', host: '127.0.0.1', port: DEFAULT_SOCKS5_PORT }), 'socks5')
  })

  test('the default SOCKS5 port is the local Tor port', () => {
    assert.equal(DEFAULT_SOCKS5_PORT, 9050)
  })
})

describe('proxy host', () => {
  test('hosts a proxy can be dialled at are accepted', () => {
    for (const host of ['127.0.0.1', 'localhost', 'proxy.example.com', '10.0.0.7', ' proxy.example.com ']) {
      assert.equal(proxyHostProblem(host), null, host)
    }
  })

  test('an IPv6 address is accepted bare or bracketed', () => {
    assert.equal(proxyHostProblem('::1'), null)
    assert.equal(proxyHostProblem('[::1]'), null)
    assert.equal(proxyHostProblem('2001:db8::7'), null)
  })

  test('an empty host asks for one', () => {
    assert.equal(proxyHostProblem(''), 'Enter the proxy host.')
    assert.equal(proxyHostProblem('   '), 'Enter the proxy host.')
  })

  test('the length limit is measured in UTF-8 bytes', () => {
    assert.equal(proxyHostProblem('a'.repeat(253)), null)
    assert.equal(proxyHostProblem('a'.repeat(254)), 'That host is too long.')
    // 127 two-byte characters fit in 254 bytes but only 127 UTF-16 units.
    assert.equal(proxyHostProblem('é'.repeat(127)), 'That host is too long.')
  })

  test('a scheme, credentials, path or inner space is refused', () => {
    for (const host of ['socks5://127.0.0.1', 'user:pass@127.0.0.1', '127.0.0.1/path', 'bad host', 'a\tb']) {
      assert.equal(proxyHostProblem(host), SHAPE, host)
    }
  })

  test('a host the URL parser rejects is not valid', () => {
    for (const host of ['[example.com]', '127.0.0.1:9050', 'exa<mple', 'a\\b', '[::1']) {
      assert.equal(proxyHostProblem(host), INVALID, host)
    }
  })

  test.todo(
    "a host carrying a query or fragment is refused (bug: '?' and '#' end the URL authority, so " +
      "'proxy.example.com?x' and 'proxy#x' pass the inline check while UpstreamProxy::validate refuses them)",
  )
})

describe('proxy port', () => {
  test('a port from 1 to 65535 is accepted', () => {
    for (const port of ['1', '9050', '65535', ' 1080 ', '0080']) {
      assert.equal(proxyPortProblem(port), null, port)
    }
  })

  test('an empty port asks for one', () => {
    assert.equal(proxyPortProblem(' '), 'Enter the proxy port.')
  })

  test('anything but digits is not a number', () => {
    for (const port of ['80a', '-1', '+80', '8.5', '1e3', 'ninety']) {
      assert.equal(proxyPortProblem(port), 'The port must be a number.', port)
    }
  })

  test('a port outside 1 to 65535 is refused', () => {
    for (const port of ['0', '65536', '99999999999999999999']) {
      assert.equal(proxyPortProblem(port), 'The port must be between 1 and 65535.', port)
    }
  })
})
