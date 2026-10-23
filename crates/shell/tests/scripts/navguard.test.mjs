// navguard.rs: NAVGUARD_JS, which stops a plain click on a risky-looking link and posts it to wisNavGuard.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'
import { h, handlers, page } from '../support/s2-dom.mjs'
import { crateSource } from '../support/s2-rust.mjs'

const RAW = rawStringConst('navguard.rs', 'NAVGUARD_JS')

// `navguard_js()` fills the placeholder with the core crate's TEXT_DOMAIN_TLDS
// as a JSON array; read the list from the same source.
const TLD_SOURCE = crateSource('core/src/navigation/link_safety.rs')
const TLD_ARRAY = TLD_SOURCE.slice(TLD_SOURCE.indexOf('pub const TEXT_DOMAIN_TLDS'))
const TLDS = [...TLD_ARRAY.slice(TLD_ARRAY.indexOf('['), TLD_ARRAY.indexOf('];')).matchAll(/"([^"]+)"/g)].map(
  (m) => m[1],
)
const SCRIPT = RAW.replace('__WIS_TEXT_TLDS__', JSON.stringify(TLDS))

/** A page at `url` with one link to `href` reading `text`, running the guard. */
function loadPage(
  href,
  text = 'Read more',
  { url = 'https://page.test/news?id=1', channel = handlers('wisNavGuard'), target } = {},
) {
  const inner = h('b', {}, text)
  const attrs = target == null ? { href } : { href, target }
  const link = href == null ? h('a', {}, inner) : h('a', attrs, inner)
  const document = page(url, h('p', {}, link))
  const window = channel ? { webkit: channel.webkit } : {}
  const run = () => new Function('window', 'document', 'location', SCRIPT)(window, document, document.location)
  run()
  return {
    window,
    document,
    link,
    inner,
    run,
    sent: () => (channel ? channel.sent.map((m) => m.body) : []),
    click: (props = {}, target = link) => document.fire('click', { target, ...props }),
  }
}

/** Whether a plain click on `href` reading `text` is stopped and posted. */
function guarded(href, text, options) {
  const p = loadPage(href, text, options)
  const click = p.click()
  const posted = p.sent().length === 1
  assert.equal(click.defaultPrevented, posted, 'a guarded click is cancelled, a safe one is not')
  return posted
}

describe('NAVGUARD_JS posting', () => {
  test('a guarded click is cancelled and posts the absolute href with the link text', () => {
    const p = loadPage('https://evil.test/login', 'paypal.com')
    const click = p.click()
    assert.deepEqual(p.sent(), [{ href: 'https://evil.test/login', text: 'paypal.com', newTab: false }])
    assert.ok(click.defaultPrevented && click.propagationStopped)
  })

  test('a link that targets a new browsing context says so, so it opens in a tab', () => {
    for (const [target, newTab] of [
      ['_blank', true],
      ['results', true],
      ['_self', false],
      ['_TOP', false],
      ['_parent', false],
      ['', false],
    ]) {
      const p = loadPage('https://evil.test/login', 'paypal.com', { target })
      p.click()
      assert.equal(p.sent()[0].newTab, newTab, target)
    }
  })

  test('a click the page dispatched itself is left to the native guard', () => {
    const p = loadPage('https://evil.test/login', 'paypal.com')
    const click = p.click({ isTrusted: false })
    assert.deepEqual(p.sent(), [])
    assert.equal(click.defaultPrevented, false)
  })

  test('link text is whitespace-collapsed and capped at 200 characters', () => {
    const p = loadPage('http://192.168.1.1/', `  Visit \n\n  the\trouter  ${'x'.repeat(300)}`)
    p.click()
    assert.equal(p.sent()[0].text.length, 200)
    assert.match(p.sent()[0].text, /^Visit the router x+$/)
  })

  test('a click on text inside the link finds the link', () => {
    const p = loadPage('http://192.168.1.1/')
    p.click({}, p.inner.childNodes[0])
    assert.equal(p.sent().length, 1)
  })

  test('Option+click and other buttons are left for Peek and the browser', () => {
    const p = loadPage('http://192.168.1.1/')
    assert.equal(p.click({ altKey: true }).defaultPrevented, false)
    assert.equal(p.click({ button: 1 }).defaultPrevented, false)
    assert.deepEqual(p.sent(), [])
  })

  test('Cmd and Ctrl clicks on a risky link are still guarded', () => {
    const p = loadPage('http://192.168.1.1/')
    p.click({ metaKey: true })
    p.click({ ctrlKey: true })
    assert.equal(p.sent().length, 2)
  })

  test('clicks outside a link, or on an anchor without href, are ignored', () => {
    const bare = loadPage(null, 'paypal.com')
    assert.equal(bare.click().defaultPrevented, false)
    const p = loadPage('http://192.168.1.1/')
    p.click({}, p.document.body)
    p.click({}, null)
    assert.deepEqual(p.sent(), [])
  })

  test('without the wisNavGuard handler the click fails quietly', () => {
    const p = loadPage('http://192.168.1.1/', 'router', { channel: null })
    assert.doesNotThrow(() => p.click())
  })

  test('installing twice registers one listener', () => {
    const p = loadPage('http://192.168.1.1/')
    p.run()
    p.click()
    assert.equal(p.sent().length, 1)
  })
})

describe('NAVGUARD_JS coarse risk check', () => {
  test('an ordinary link is left to navigate', () => {
    assert.equal(guarded('https://example.com/about', 'About us'), false)
  })

  test('credentials in the URL are guarded', () => {
    assert.equal(guarded('https://user:pw@example.com/'), true)
    assert.equal(guarded('https://user@example.com/'), true)
  })

  test('raw IPv4 hosts are guarded', () => {
    assert.equal(guarded('http://192.168.1.1/admin'), true)
  })

  test('punycode labels are guarded', () => {
    assert.equal(guarded('https://xn--pypal-4ve.com/'), true)
    assert.equal(guarded('https://login.xn--80ak6aa92e.com/'), true)
    assert.equal(guarded('https://pаypal.com/'), true, 'a Cyrillic lookalike reaches the check as punycode')
  })

  test('hosts with more than five labels are guarded, not counting a leading www', () => {
    assert.equal(guarded('https://a.b.c.d.com/'), false)
    assert.equal(guarded('https://a.b.c.d.e.com/'), true)
    assert.equal(guarded('https://www.a.b.c.d.com/'), false)
  })

  test('text naming another site is guarded', () => {
    assert.equal(guarded('https://evil.test/', 'paypal.com'), true)
    assert.equal(guarded('https://paypal.com.evil.io/', 'paypal.com/signin'), true)
  })

  test('text naming the same site, a parent of the host, or its www form is fine', () => {
    assert.equal(guarded('https://paypal.com/', 'www.PayPal.com'), false)
    assert.equal(guarded('https://login.paypal.com/', 'paypal.com'), false)
    assert.equal(guarded('https://www.paypal.com/', 'paypal.com'), false)
  })

  test('text with a scheme is read as a URL', () => {
    assert.equal(guarded('https://evil.test/', 'https://www.paypal.com/login'), true)
    assert.equal(guarded('https://paypal.com/', 'https://www.paypal.com/login'), false)
  })

  test('text that only looks dotted does not name a site', () => {
    for (const text of ['README.md', 'Cargo.toml', 'Click here.', 'go to paypal.com now', 'v1.2', '']) {
      assert.equal(guarded('https://example.com/', text), false, text)
    }
  })

  test('every TLD in the shared list makes dotted text name a site', () => {
    for (const tld of TLDS) {
      assert.equal(guarded('https://example.org/', `brand.${tld}`), true, tld)
    }
  })

  test('non-web schemes are never guarded', () => {
    assert.equal(guarded('mailto:a@paypal.com', 'paypal.com'), false)
    assert.equal(guarded('javascript:void(0)', 'paypal.com'), false)
    assert.equal(guarded('ftp://192.168.1.1/'), false)
  })

  test('a fragment link within the current page is never guarded', () => {
    const url = 'http://192.168.1.1/page?x=1'
    assert.equal(guarded('#top', 'paypal.com', { url }), false)
    assert.equal(guarded('http://192.168.1.1/other#top', 'x', { url }), true, 'another page on the same host')
  })

  test('a link whose href cannot be parsed is not guarded', () => {
    assert.equal(guarded('http://[bad/', 'paypal.com'), false)
  })

  test('a host that is empty once www. is stripped is not guarded', () => {
    assert.equal(guarded('https://www./', 'paypal.com'), false)
  })
})
