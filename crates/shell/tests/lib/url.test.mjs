// Address-bar input resolution, search engines, link safety and tab names (`lib/url.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import {
  activeEngine,
  DOWNLOADS_URL,
  HISTORY_URL,
  isChromeUrl,
  isSearchQuery,
  isWebUrl,
  NEW_TAB_URL,
  normaliseUrl,
  resolveInput,
  resolveNewTabUrl,
  SEARCH_ENGINES,
  SETTINGS_URL,
  SOURCE_URL,
  sourceTargetOf,
  sourceUrlFor,
  tabDisplayName,
} from '../../src/lib/url.ts'

const DDG = 'https://duckduckgo.com/?q='

const searchSettings = (overrides = {}) => ({
  search_engine: 'duckduckgo',
  custom_search_engines: [],
  builtin_search_keywords: {},
  ...overrides,
})

const kagi = (overrides = {}) => ({
  name: 'Kagi',
  keyword: 'k',
  template: 'https://kagi.com/search?q={searchTerms}',
  is_default: false,
  ...overrides,
})

const page = (url) => ({ url, isSearch: false, engineLabel: null })
const search = (url, engineLabel) => ({ url, isSearch: true, engineLabel })

describe('address bar input', () => {
  test('blank input resolves to nothing', () => {
    assert.equal(resolveInput(''), null)
    assert.equal(resolveInput('   \t '), null)
  })

  test('a bare host gets https', () => {
    assert.deepEqual(resolveInput('example.com'), page('https://example.com'))
    assert.deepEqual(resolveInput('  example.com  '), page('https://example.com'))
  })

  test('a host keeps its port and path', () => {
    assert.deepEqual(resolveInput('example.com:8080/a/b?c=1'), page('https://example.com:8080/a/b?c=1'))
    assert.deepEqual(resolveInput('192.168.1.1:8080'), page('https://192.168.1.1:8080'))
  })

  test('a full URL is kept, normalised by the URL parser', () => {
    assert.deepEqual(resolveInput('HTTPS://Example.COM'), page('https://example.com/'))
    assert.deepEqual(resolveInput('http://example.com/a b'), page('http://example.com/a%20b'))
  })

  test('about: pages are navigable', () => {
    assert.deepEqual(resolveInput('about:blank'), page('about:blank'))
  })

  test('other schemes are searched for, never opened', () => {
    for (const raw of [
      'javascript:alert(document.cookie)',
      'file:///etc/hosts.txt',
      'mailto:someone@example.com',
      'data:text/html,<b>x</b>',
    ]) {
      const resolved = resolveInput(raw)
      assert.equal(resolved.isSearch, true, raw)
      assert.equal(resolved.url, DDG + encodeURIComponent(raw), raw)
    }
  })

  test('a query with spaces is a search', () => {
    assert.deepEqual(
      resolveInput('rust borrow checker'),
      search(`${DDG}rust%20borrow%20checker`, 'DuckDuckGo'),
    )
  })

  test('a single word without a dot is a search', () => {
    assert.deepEqual(resolveInput('weather'), search(`${DDG}weather`, 'DuckDuckGo'))
  })

  test('search terms are URL-encoded', () => {
    assert.equal(resolveInput('c++ & rust?').url, `${DDG}c%2B%2B%20%26%20rust%3F`)
  })

  test('a keyword runs the query on its custom engine', () => {
    const settings = searchSettings({ custom_search_engines: [kagi()] })
    assert.deepEqual(
      resolveInput('k rust traits', settings),
      search('https://kagi.com/search?q=rust%20traits', 'Kagi'),
    )
  })

  test('a keyword matches case-insensitively and outranks the host heuristic', () => {
    const settings = searchSettings({ custom_search_engines: [kagi()] })
    assert.deepEqual(
      resolveInput('K   example.com', settings),
      search('https://kagi.com/search?q=example.com', 'Kagi'),
    )
  })

  test('a built-in engine can be given a keyword', () => {
    const settings = searchSettings({ builtin_search_keywords: { google: 'g' } })
    assert.deepEqual(
      resolveInput('g cats', settings),
      search('https://www.google.com/search?q=cats', 'Google'),
    )
  })

  test('a custom engine keyword wins over a built-in one', () => {
    const settings = searchSettings({
      custom_search_engines: [kagi({ keyword: 'g' })],
      builtin_search_keywords: { google: 'g' },
    })
    assert.equal(resolveInput('g cats', settings).engineLabel, 'Kagi')
  })

  test('a keyword stored for an unknown engine is ignored', () => {
    const settings = searchSettings({ builtin_search_keywords: { yahoo: 'y' } })
    assert.deepEqual(resolveInput('y cats', settings), search(`${DDG}y%20cats`, 'DuckDuckGo'))
  })

  test('a bare keyword with no query falls through to a normal search', () => {
    const settings = searchSettings({ custom_search_engines: [kagi()] })
    assert.deepEqual(resolveInput('k   ', settings), search(`${DDG}k`, 'DuckDuckGo'))
  })

  test('a first word that is no keyword stays part of the query', () => {
    const settings = searchSettings({ custom_search_engines: [kagi()] })
    assert.equal(resolveInput('what is rust', settings).url, `${DDG}what%20is%20rust`)
  })

  test('the default custom engine handles plain searches', () => {
    const settings = searchSettings({ search_engine: 'bing', custom_search_engines: [kagi({ is_default: true })] })
    assert.deepEqual(resolveInput('cats', settings), search('https://kagi.com/search?q=cats', 'Kagi'))
  })

  test('the chosen built-in engine searches when no custom engine is the default', () => {
    const settings = searchSettings({ search_engine: 'bing', custom_search_engines: [kagi()] })
    assert.deepEqual(resolveInput('cats', settings), search('https://www.bing.com/search?q=cats', 'Bing'))
  })

  test('every placeholder in a template is filled', () => {
    const settings = searchSettings({
      custom_search_engines: [kagi({ template: 'https://x.test/?q={searchTerms}&again={searchTerms}' })],
    })
    assert.equal(resolveInput('k a b', settings).url, 'https://x.test/?q=a%20b&again=a%20b')
  })
})

describe('search engines', () => {
  test('without settings DuckDuckGo searches', () => {
    assert.equal(activeEngine().label, 'DuckDuckGo')
    assert.equal(resolveInput('cats').engineLabel, 'DuckDuckGo')
  })

  test('an unrecognised stored engine falls back to DuckDuckGo', () => {
    assert.equal(activeEngine(searchSettings({ search_engine: 'yahoo' })).label, 'DuckDuckGo')
  })

  test('each built-in engine builds its own query URL', () => {
    assert.equal(SEARCH_ENGINES.duckduckgo.search('a b'), `${DDG}a%20b`)
    assert.equal(SEARCH_ENGINES.google.search('a b'), 'https://www.google.com/search?q=a%20b')
    assert.equal(SEARCH_ENGINES.bing.search('a b'), 'https://www.bing.com/search?q=a%20b')
  })
})

describe('normalising a typed URL', () => {
  test('blank input normalises to nothing', () => {
    assert.equal(normaliseUrl('  '), null)
  })

  test('a bare host gets https', () => {
    assert.equal(normaliseUrl('example.com/path'), 'https://example.com/path')
  })

  test('a web or about: URL is kept', () => {
    assert.equal(normaliseUrl('http://Example.com'), 'http://example.com/')
    assert.equal(normaliseUrl('about:blank'), 'about:blank')
  })

  test('anything else searches with the given engine', () => {
    assert.equal(normaliseUrl('cats'), `${DDG}cats`)
    assert.equal(normaliseUrl('cats', 'google'), 'https://www.google.com/search?q=cats')
    assert.equal(normaliseUrl('mailto:a@b.co', 'bing'), 'https://www.bing.com/search?q=mailto%3Aa%40b.co')
  })

  test('an unknown engine falls back to DuckDuckGo', () => {
    assert.equal(normaliseUrl('cats', 'yahoo'), `${DDG}cats`)
  })
})

describe('telling a search from an address', () => {
  test('blank input is not a search', () => {
    assert.equal(isSearchQuery('   '), false)
  })

  test('URLs and hosts are not searches; words and other schemes are', () => {
    assert.equal(isSearchQuery('https://example.com'), false)
    assert.equal(isSearchQuery('about:settings'), false)
    assert.equal(isSearchQuery('example.com'), false)
    assert.equal(isSearchQuery('rust traits'), true)
    assert.equal(isSearchQuery('mailto:a@b.co'), true)
  })

  test('agrees with what normalising the same input does', () => {
    for (const raw of ['example.com', 'http://x.test', 'about:blank', 'cats', 'a.b c', 'javascript:x.y', '10.0.0.1:80']) {
      assert.equal(isSearchQuery(raw), normaliseUrl(raw).startsWith(DDG), raw)
    }
  })
})

describe('link safety', () => {
  test('http and https links are web links', () => {
    assert.equal(isWebUrl('https://example.com/a'), true)
    assert.equal(isWebUrl('HTTP://example.com'), true)
  })

  test('every other scheme is refused', () => {
    for (const href of ['javascript:alert(1)', 'data:text/html,x', 'file:///etc/passwd', 'about:blank', 'mailto:a@b.co']) {
      assert.equal(isWebUrl(href), false, href)
    }
  })

  test('a scheme disguised with whitespace is still refused', () => {
    assert.equal(isWebUrl(' javascript:alert(1)'), false)
    assert.equal(isWebUrl('java\tscript:alert(1)'), false)
  })

  test('relative and unparseable links are refused', () => {
    assert.equal(isWebUrl('/path'), false)
    assert.equal(isWebUrl(''), false)
    assert.equal(isWebUrl('https://'), false)
  })

  test('about: URLs are chrome pages', () => {
    assert.equal(isChromeUrl(NEW_TAB_URL), true)
    assert.equal(isChromeUrl(SETTINGS_URL), true)
    assert.equal(isChromeUrl('https://about.com'), false)
  })
})

describe('the new-tab page', () => {
  test('the launchpad opens before settings have loaded', () => {
    assert.equal(resolveNewTabUrl(undefined), NEW_TAB_URL)
  })

  test('the homepage opens when chosen and set', () => {
    assert.equal(resolveNewTabUrl({ new_tab_target: 'homepage', homepage: 'https://home.test' }), 'https://home.test')
  })

  test('the launchpad opens when the homepage is chosen but empty', () => {
    assert.equal(resolveNewTabUrl({ new_tab_target: 'homepage', homepage: '' }), NEW_TAB_URL)
  })

  test('the launchpad opens when chosen, even with a homepage set', () => {
    assert.equal(resolveNewTabUrl({ new_tab_target: 'launchpad', homepage: 'https://home.test' }), NEW_TAB_URL)
  })
})

describe('source tabs', () => {
  test('the page and tab survive a round trip, special characters included', () => {
    const target = 'https://example.com/a?b=1&c=2#frag'
    assert.deepEqual(sourceTargetOf(sourceUrlFor(target, 'tab 1&2')), { url: target, tab: 'tab 1&2' })
  })

  test('a bare source URL shows nothing', () => {
    assert.deepEqual(sourceTargetOf(SOURCE_URL), { url: '', tab: '' })
  })

  test('other URLs are not source tabs', () => {
    for (const url of ['about:sourcery', 'about:settings', 'https://example.com/?url=x&tab=y']) {
      assert.equal(sourceTargetOf(url), null, url)
    }
  })
})

describe('tab names', () => {
  const named = (title, url) => tabDisplayName({ title, url })

  test('a source tab is named Page source', () => {
    assert.equal(named('view-source', sourceUrlFor('https://x.test', 't')), 'Page source')
  })

  test('a title without site separators is kept, trimmed', () => {
    assert.equal(named('  Inbox  ', 'https://mail.test/'), 'Inbox')
    assert.equal(named('Rust-based tools', 'https://x.test/'), 'Rust-based tools')
  })

  test('the site name is dropped from the end of a title', () => {
    assert.equal(named('Rust traits explained - YouTube', 'https://www.youtube.com/watch?v=1'), 'Rust traits explained')
    assert.equal(
      named('Rust (programming language) - Wikipedia', 'https://en.wikipedia.org/wiki/Rust'),
      'Rust (programming language)',
    )
  })

  test('the brand matches ignoring case, spaces and punctuation', () => {
    assert.equal(named('Some question | Stack Overflow', 'https://stackoverflow.com/q/1'), 'Some question')
  })

  test('the longest remaining segment wins when the brand is not first', () => {
    assert.equal(
      named('Pull requests · rust-lang/rust · GitHub', 'https://github.com/rust-lang/rust/pulls'),
      'rust-lang/rust',
    )
  })

  test('a title that leads with the brand keeps the brand', () => {
    assert.equal(named('GitHub - torvalds/linux', 'https://github.com/torvalds/linux'), 'GitHub')
  })

  test('without a brand match the first segment is used', () => {
    assert.equal(named('Post title | Hacker News', 'https://news.ycombinator.com/item'), 'Post title')
    assert.equal(named('A - B', 'not a url'), 'A')
    assert.equal(named('A - B', 'about:blank'), 'A')
  })

  test('a single-label host is its own brand', () => {
    assert.equal(named('Admin - Localhost - Dashboard overview', 'http://localhost:3000/'), 'Dashboard overview')
  })

  test.todo(
    "a hyphenated domain's brand is recognised in the title (bug: matchesBrand strips punctuation from the " +
      "title segment but not from the brand token, so 'rust-lang' never matches; 'Intro - Rust-Lang - The Rust " +
      "Programming Language Book' on rust-lang.org shows 'Intro')",
  )

  test('an untitled page is named by its host, without www.', () => {
    assert.equal(named('', 'https://www.example.com/a'), 'example.com')
    assert.equal(named('https://example.com:8080/a', 'https://example.com:8080/a'), 'example.com:8080')
  })

  test('untitled chrome pages get their own names', () => {
    assert.equal(named('', SETTINGS_URL), 'Settings')
    assert.equal(named(HISTORY_URL, HISTORY_URL), 'History')
    assert.equal(named('', DOWNLOADS_URL), 'Downloads')
    assert.equal(named('', NEW_TAB_URL), 'Launchpad')
  })

  test('an untitled tab with an unparseable URL shows the URL', () => {
    assert.equal(named('', 'not a url'), 'not a url')
  })
})
