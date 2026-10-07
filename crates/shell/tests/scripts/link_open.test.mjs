// link_open.rs: OPEN_IN_TAB_JS (modified or middle click to a background tab) and PANEL_CROSS_SITE_JS (panel cross-site click to a foreground tab).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'
import { h, handlers, page } from '../support/s2-dom.mjs'

const OPEN_IN_TAB = rawStringConst('link_open.rs', 'OPEN_IN_TAB_JS')
const PANEL = rawStringConst('link_open.rs', 'PANEL_CROSS_SITE_JS')

/** A page at `url` holding `children`, running `script`. */
function loadPage(script, { url = 'https://page.test/article', children = [], channel = handlers('wisOpenInTab'), navigator = {} } = {}) {
  const document = page(url, ...children)
  const window = channel ? { webkit: channel.webkit } : {}
  const run = () =>
    new Function('window', 'document', 'location', 'navigator', script)(window, document, document.location, navigator)
  run()
  return {
    window,
    document,
    run,
    sent: () => (channel ? channel.sent : []),
    fire: (type, target, props = {}) => document.fire(type, { target, ...props }),
  }
}

/** A link and the page around it. */
function linkPage(script, href, options = {}) {
  const inner = h('span', {}, 'label')
  const link = h('a', { href }, inner, ' tail')
  const p = loadPage(script, { ...options, children: [h('p', {}, link)] })
  return { ...p, link, inner, text: link.childNodes[1] }
}

describe('OPEN_IN_TAB_JS', () => {
  test('Cmd+click on a link posts its absolute href and stops the navigation', () => {
    const p = linkPage(OPEN_IN_TAB, '/docs?x=1')
    const click = p.fire('click', p.link, { metaKey: true })
    assert.deepEqual(p.sent(), [{ name: 'wisOpenInTab', body: 'https://page.test/docs?x=1' }])
    assert.ok(click.defaultPrevented && click.propagationStopped)
  })

  test('Ctrl+click does the same, for Windows and Linux', () => {
    const p = linkPage(OPEN_IN_TAB, 'https://other.test/')
    p.fire('click', p.link, { ctrlKey: true })
    assert.deepEqual(p.sent(), [{ name: 'wisOpenInTab', body: 'https://other.test/' }])
  })

  test('a click on an element or text inside the link finds the link', () => {
    const p = linkPage(OPEN_IN_TAB, '/a')
    p.fire('click', p.inner, { metaKey: true })
    p.fire('click', p.text, { metaKey: true })
    assert.deepEqual(
      p.sent().map((m) => m.body),
      ['https://page.test/a', 'https://page.test/a'],
    )
  })

  test('plain clicks, Option clicks and Cmd+Option clicks are left alone', () => {
    const p = linkPage(OPEN_IN_TAB, '/a')
    for (const keys of [{}, { altKey: true }, { metaKey: true, altKey: true }, { shiftKey: true }]) {
      const click = p.fire('click', p.link, keys)
      assert.equal(click.defaultPrevented, false)
    }
    assert.deepEqual(p.sent(), [])
  })

  test('a Cmd+click with another button is ignored by the click handler', () => {
    const p = linkPage(OPEN_IN_TAB, '/a')
    p.fire('click', p.link, { metaKey: true, button: 2 })
    assert.deepEqual(p.sent(), [])
  })

  test('a middle-click (auxclick, button 1) on a link posts it and stops the navigation', () => {
    const p = linkPage(OPEN_IN_TAB, '/m')
    const aux = p.fire('auxclick', p.link, { button: 1 })
    assert.deepEqual(p.sent(), [{ name: 'wisOpenInTab', body: 'https://page.test/m' }])
    assert.ok(aux.defaultPrevented && aux.propagationStopped)
  })

  test('a right-button auxclick is ignored', () => {
    const p = linkPage(OPEN_IN_TAB, '/m')
    const aux = p.fire('auxclick', p.link, { button: 2 })
    assert.deepEqual(p.sent(), [])
    assert.equal(aux.defaultPrevented, false)
  })

  test('a middle-button press on a link is cancelled to stop autoscroll, other presses are not', () => {
    const p = linkPage(OPEN_IN_TAB, '/m')
    assert.equal(p.fire('mousedown', p.link, { button: 1 }).defaultPrevented, true)
    assert.equal(p.fire('mousedown', p.link, { button: 0 }).defaultPrevented, false)
    assert.equal(p.fire('mousedown', p.document.body, { button: 1 }).defaultPrevented, false)
    assert.deepEqual(p.sent(), [], 'a press alone opens nothing')
  })

  test('non-web links are left to the page', () => {
    for (const href of ['mailto:a@b.test', 'javascript:void(0)', 'ftp://files.test/x', 'tel:123']) {
      const p = linkPage(OPEN_IN_TAB, href)
      const click = p.fire('click', p.link, { metaKey: true })
      const aux = p.fire('auxclick', p.link, { button: 1 })
      const press = p.fire('mousedown', p.link, { button: 1 })
      assert.deepEqual(p.sent(), [], href)
      assert.equal(click.defaultPrevented || aux.defaultPrevented || press.defaultPrevented, false, href)
    }
  })

  test('clicks outside a link, on an anchor without href, or with no element target are ignored', () => {
    const bare = h('a', { name: 'top' }, 'anchor')
    const p = loadPage(OPEN_IN_TAB, { children: [bare, h('p', {}, 'text')] })
    for (const target of [bare, p.document.body, p.document, null]) {
      p.fire('click', target, { metaKey: true })
      p.fire('auxclick', target, { button: 1 })
    }
    assert.deepEqual(p.sent(), [])
  })

  test('without the handler the click fails quietly', () => {
    const p = linkPage(OPEN_IN_TAB, '/a', { channel: null })
    assert.doesNotThrow(() => p.fire('click', p.link, { metaKey: true }))
    assert.doesNotThrow(() => p.fire('auxclick', p.link, { button: 1 }))
  })

  test('a handler that throws is contained', () => {
    const p = linkPage(OPEN_IN_TAB, '/a')
    p.window.webkit.messageHandlers.wisOpenInTab.postMessage = () => {
      throw new Error('gone')
    }
    assert.doesNotThrow(() => p.fire('click', p.link, { metaKey: true }))
  })

  test('a Cmd+click or middle-click the page dispatched itself is left to the page', () => {
    const p = linkPage(OPEN_IN_TAB, '/a')
    const click = p.fire('click', p.link, { metaKey: true, isTrusted: false })
    const aux = p.fire('auxclick', p.link, { button: 1, isTrusted: false })
    assert.deepEqual(p.sent(), [])
    assert.ok(!click.defaultPrevented && !aux.defaultPrevented)
  })

  test('installing twice registers the listeners once', () => {
    const p = linkPage(OPEN_IN_TAB, '/a')
    p.run()
    p.fire('click', p.link, { metaKey: true })
    assert.equal(p.sent().length, 1)
    assert.deepEqual(
      ['click', 'auxclick', 'mousedown'].map((t) => p.document.listenerCount(t)),
      [1, 1, 1],
    )
  })
})

/** A widget panel at `url` with one link to `href`. */
function panelLink(href, { url = 'https://mail.google.com/inbox', channel = handlers('wisOpenInTabFg'), attrs = {}, navigator } = {}) {
  const link = h('a', { href, ...attrs }, 'link')
  const p = loadPage(PANEL, { url, channel, children: [link], navigator })
  return { ...p, link, click: (props = {}) => p.fire('click', link, props) }
}

describe('PANEL_CROSS_SITE_JS', () => {
  test('a plain click on a link to another site posts it to wisOpenInTabFg and stops the navigation', () => {
    const p = panelLink('https://news.test/story')
    const click = p.click()
    assert.deepEqual(p.sent(), [{ name: 'wisOpenInTabFg', body: 'https://news.test/story' }])
    assert.ok(click.defaultPrevented && click.propagationStopped)
  })

  test('a click the page dispatched opens a tab only inside a user gesture', () => {
    const idle = panelLink('https://news.test/story')
    assert.equal(idle.click({ isTrusted: false }).defaultPrevented, false)
    assert.deepEqual(idle.sent(), [])

    const gesture = panelLink('https://news.test/story', { navigator: { userActivation: { isActive: true } } })
    gesture.click({ isTrusted: false })
    assert.deepEqual(gesture.sent(), [{ name: 'wisOpenInTabFg', body: 'https://news.test/story' }])
  })

  test('a link within the same registrable site stays in the panel', () => {
    const p = panelLink('https://accounts.google.com/x')
    const click = p.click()
    assert.deepEqual(p.sent(), [])
    assert.equal(click.defaultPrevented, false)
  })

  test('a relative link stays in the panel', () => {
    const p = panelLink('/settings')
    p.click()
    assert.deepEqual(p.sent(), [])
  })

  test('ccSLD hosts compare one label deeper', () => {
    const url = 'https://news.bbc.co.uk/'
    const same = panelLink('https://www.bbc.co.uk/sport', { url })
    same.click()
    assert.deepEqual(same.sent(), [])
    const other = panelLink('https://shop.other.co.uk/', { url })
    other.click()
    assert.equal(other.sent().length, 1)
  })

  test('a two-letter TLD without a ccSLD label compares two labels', () => {
    const url = 'https://a.example.de/'
    const same = panelLink('https://b.example.de/', { url })
    same.click()
    assert.deepEqual(same.sent(), [])
  })

  test('IP hosts must match exactly', () => {
    const url = 'http://10.0.0.1/admin'
    const same = panelLink('http://10.0.0.1/other', { url })
    same.click()
    assert.deepEqual(same.sent(), [])
    const other = panelLink('http://10.0.0.2/', { url })
    other.click()
    assert.equal(other.sent().length, 1)
  })

  test('IPv6 hosts compare as written', () => {
    const p = panelLink('http://[::1]/x', { url: 'http://[::1]:8080/' })
    p.click()
    assert.deepEqual(p.sent(), [])
  })

  test('a trailing dot and letter case do not make a host another site', () => {
    const p = panelLink('https://EXAMPLE.com/a', { url: 'https://www.example.com./' })
    p.click()
    assert.deepEqual(p.sent(), [])
  })

  test('modifier clicks and other buttons are left alone', () => {
    const p = panelLink('https://news.test/')
    for (const props of [{ metaKey: true }, { ctrlKey: true }, { altKey: true }, { shiftKey: true }, { button: 1 }]) {
      assert.equal(p.click(props).defaultPrevented, false)
    }
    assert.deepEqual(p.sent(), [])
  })

  test('download links and non-web links are left alone', () => {
    const download = panelLink('https://cdn.test/file.zip', { attrs: { download: '' } })
    download.click()
    assert.deepEqual(download.sent(), [])
    const mail = panelLink('mailto:a@b.test')
    mail.click()
    assert.deepEqual(mail.sent(), [])
  })

  test('a panel page without a hostname lets every link through', () => {
    const p = panelLink('https://news.test/', { url: 'about:blank' })
    const click = p.click()
    assert.deepEqual(p.sent(), [])
    assert.equal(click.defaultPrevented, false)
  })

  test('a link whose host cannot be parsed is left alone', () => {
    const p = panelLink('http://[bad/')
    const click = p.click()
    assert.deepEqual(p.sent(), [])
    assert.equal(click.defaultPrevented, false)
  })

  test('clicks outside a link or on text inside one are handled like the link', () => {
    const p = panelLink('https://news.test/')
    p.fire('click', p.document.body)
    p.fire('click', null)
    assert.deepEqual(p.sent(), [])
    p.fire('click', p.link.childNodes[0])
    assert.equal(p.sent().length, 1)
  })

  test('without the foreground handler the click fails quietly', () => {
    const p = panelLink('https://news.test/', { channel: null })
    assert.doesNotThrow(() => p.click())
  })

  test('installing twice registers the listener once', () => {
    const p = panelLink('https://news.test/')
    p.run()
    p.click()
    assert.equal(p.sent().length, 1)
  })
})
