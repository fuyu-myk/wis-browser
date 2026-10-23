// The command palette's keymap, fuzzy ranking and row sources (`lib/palette.ts`).

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, test } from 'node:test'

import { normalizeAccelerator } from '../../src/lib/keybindings.ts'
import {
  actionLabel,
  allActions,
  DEFAULT_KEYMAP,
  effectiveKeymap,
  fuzzyMatch,
  historyKey,
  makeActionsSource,
  makeBookmarksSource,
  makeClosedTabsSource,
  makeHistorySource,
  makeNavigateSource,
  makeSettingsTogglesSource,
  makeTabsSource,
} from '../../src/lib/palette.ts'
import { evaluateOn, WINDOWS_UA } from '../support/l1-platform.mjs'

// Node's user agent names no OS, which the platform module reads as macOS.

/** Records each call's arguments, so a row's activation can be asserted. */
function recorder() {
  const calls = []
  const fn = (...args) => {
    calls.push(args)
  }
  fn.calls = calls
  return fn
}

const ids = (items) => items.map((it) => it.id)

describe('keymap', () => {
  test('the defaults mirror default_keymap_for in the Rust core', () => {
    const rust = readFileSync(new URL('../../../core/src/settings/keybinding.rs', import.meta.url), 'utf8')
    const body = rust.slice(rust.indexOf('pub fn default_keymap_for'), rust.indexOf('pub fn effective_keymap'))
    const pairs = [...body.matchAll(/\(KeyAction::(\w+), "([^"]+)"\)/g)].map(([, variant, accel]) => [
      variant.replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase(),
      accel,
    ])
    assert.ok(pairs.length > 40, 'parsed the Rust table')
    assert.deepEqual(DEFAULT_KEYMAP, Object.fromEntries(pairs))
  })

  test('no two default actions share a keystroke', () => {
    const seen = new Map()
    for (const [action, accel] of Object.entries(DEFAULT_KEYMAP)) {
      const key = normalizeAccelerator(accel)
      assert.equal(seen.get(key), undefined, `${action} collides with ${seen.get(key)} on ${accel}`)
      seen.set(key, action)
    }
  })

  test('Windows and Linux defaults use Ctrl where macOS uses Cmd', () => {
    const [newTab, closePane, anyCmd, closeShortcut] = evaluateOn(
      WINDOWS_UA,
      'palette.ts',
      `[
        m.DEFAULT_KEYMAP.new_tab,
        m.DEFAULT_KEYMAP.close_pane,
        Object.values(m.DEFAULT_KEYMAP).some((a) => a.includes('Cmd')),
        m.makeTabsSource({
          state: { workspaces: [], active_workspace_id: 'w', tabs: [{ id: 't', title: 'T', url: 'https://t.test/' }] },
          onActivate() {},
          onClose() {},
        })('')[0].secondaryAction.shortcut,
      ]`,
    )
    assert.equal(newTab, 'Ctrl+T')
    assert.equal(closePane, 'Alt+Ctrl+W')
    assert.equal(anyCmd, false)
    assert.equal(closeShortcut, 'Ctrl+Backspace')
  })

  test('without overrides the defaults apply', () => {
    assert.equal(effectiveKeymap(undefined), DEFAULT_KEYMAP)
    assert.deepEqual(effectiveKeymap({}), DEFAULT_KEYMAP)
  })

  test('an override rebinds an action or binds one with no default', () => {
    const keymap = effectiveKeymap({ new_tab: 'Cmd+Shift+Y', focus_url_bar_alt: 'Cmd+K' })
    assert.equal(keymap.new_tab, 'Cmd+Shift+Y')
    assert.equal(keymap.focus_url_bar_alt, 'Cmd+K')
    assert.equal(keymap.reload, 'Cmd+R')
  })

  test('an empty override unbinds the action', () => {
    assert.equal('reload' in effectiveKeymap({ reload: '' }), false)
  })

  test('overriding never changes the defaults', () => {
    effectiveKeymap({ reload: '', new_tab: 'Cmd+Y' })
    assert.equal(DEFAULT_KEYMAP.reload, 'Cmd+R')
    assert.equal(DEFAULT_KEYMAP.new_tab, 'Cmd+T')
  })

  test('every action has a label, and every default belongs to a listed action', () => {
    const actions = allActions()
    for (const action of actions) assert.ok(actionLabel(action), action)
    for (const action of Object.keys(DEFAULT_KEYMAP)) assert.ok(actions.includes(action), action)
    assert.equal(actionLabel('close_tab'), 'Close tab')
  })
})

describe('fuzzy matching', () => {
  test('an empty query matches anything', () => {
    assert.deepEqual(fuzzyMatch('anything', ''), { score: 1, indices: [] })
  })

  test('nothing matches an empty haystack', () => {
    assert.deepEqual(fuzzyMatch('', 'a'), { score: 0, indices: [] })
  })

  test('a query that is not a subsequence misses', () => {
    assert.deepEqual(fuzzyMatch('youtube', 'tuy'), { score: 0, indices: [] })
  })

  test('matching ignores case and reports where each character landed', () => {
    const match = fuzzyMatch('YouTube', 'yt')
    assert.deepEqual(match.indices, [0, 3])
    assert.ok(match.score > 0)
  })

  test('a contiguous substring beats scattered characters', () => {
    assert.ok(fuzzyMatch('youtube', 'out').score > fuzzyMatch('youtube', 'ye').score)
  })

  test('an acronym still matches, behind a contiguous match', () => {
    const acronym = fuzzyMatch('DuckDuckGo', 'ddg').score
    assert.ok(acronym > 0)
    assert.ok(acronym < fuzzyMatch('DuckDuckGo', 'duck').score)
  })

  test('an earlier match outranks a later one', () => {
    assert.ok(fuzzyMatch('book of rust', 'book').score > fuzzyMatch('the rust book', 'book').score)
  })

  test('a scattered match far down a long string still scores', () => {
    const haystack = `${'x'.repeat(100)}a${'x'.repeat(500)}b`
    assert.equal(fuzzyMatch(haystack, 'ab').score, 1)
  })
})

describe('tabs', () => {
  const state = {
    active_workspace_id: 'w1',
    workspaces: [
      { id: 'w1', name: 'Work', accent: 'blue', tab_order: ['t1', 't2'] },
      { id: 'w2', name: 'Home', accent: 'green', tab_order: ['t3'] },
    ],
    tabs: [
      { id: 't1', title: 'Rust docs', url: 'https://doc.rust-lang.org/' },
      { id: 't2', title: '', url: 'https://example.com/' },
      { id: 't3', title: 'Recipes', url: 'https://food.test/' },
      { id: 't4', title: 'Loose', url: 'https://loose.test/' },
    ],
  }
  const source = (onActivate = recorder(), onClose = recorder()) => makeTabsSource({ state, onActivate, onClose })

  test('an empty query lists every tab', () => {
    assert.deepEqual(ids(source()('')), ['tab:t1', 'tab:t2', 'tab:t3', 'tab:t4'])
  })

  test('a query keeps only matching tabs, best first', () => {
    assert.deepEqual(ids(source()('rust')), ['tab:t1'])
    assert.deepEqual(source()('zzz'), [])
  })

  test('an untitled tab is titled by its URL', () => {
    const row = source()('example').find((it) => it.id === 'tab:t2')
    assert.equal(row.title, 'https://example.com/')
    assert.equal(row.subtitle, 'https://example.com/')
  })

  test('title and URL matches are marked separately', () => {
    const [row] = source()('docs')
    assert.deepEqual(row.titleMatches, [5, 6, 7, 8])
    assert.deepEqual(row.subtitleMatches, [8, 9, 10, 14])
  })

  test('a tab is found by its workspace name, flagged as from another workspace', () => {
    const [row] = source()('home')
    assert.equal(row.id, 'tab:t3')
    assert.equal(row.titleMatches, undefined)
    assert.equal(row.subtitleMatches, undefined)
    assert.deepEqual(row.tabMeta, { workspaceName: 'Home', workspaceAccent: 'green', crossWorkspace: true })
  })

  test('a tab in the active workspace is not cross-workspace; one in none has no workspace', () => {
    const rows = source()('')
    assert.equal(rows.find((it) => it.id === 'tab:t1').tabMeta.crossWorkspace, false)
    assert.equal(rows.find((it) => it.id === 'tab:t4').tabMeta, undefined)
  })

  test('a row switches to its tab, and its secondary action closes it with the platform shortcut', async () => {
    const onActivate = recorder()
    const onClose = recorder()
    const [row] = source(onActivate, onClose)('rust')
    await row.activate()
    await row.secondaryAction.run()
    assert.deepEqual(onActivate.calls, [[state.tabs[0]]])
    assert.deepEqual(onClose.calls, [[state.tabs[0]]])
    assert.equal(row.secondaryAction.label, 'Close tab')
    assert.equal(row.secondaryAction.shortcut, '⌘⌫')
  })
})

describe('actions', () => {
  test('an action matches by label and shows its binding', () => {
    const [row] = makeActionsSource({ keymap: DEFAULT_KEYMAP, onDispatch: recorder() })('new tab')
    assert.equal(row.id, 'action:new_tab')
    assert.equal(row.title, 'New tab')
    assert.equal(row.accessory, 'Cmd+T')
    assert.deepEqual(row.titleMatches, [0, 1, 2, 3, 4, 5, 6])
  })

  test('an unbound action shows a dash', () => {
    const [row] = makeActionsSource({ keymap: {}, onDispatch: recorder() })('new tab')
    assert.equal(row.accessory, '—')
  })

  test('positional switch-to-tab actions are left out; everything else is listed', () => {
    const rows = makeActionsSource({ keymap: DEFAULT_KEYMAP, onDispatch: recorder() })('')
    assert.equal(rows.length, allActions().length - 9)
    assert.ok(!rows.some((it) => /^action:switch_to_tab\d$/.test(it.id)))
    assert.ok(rows.some((it) => it.id === 'action:switch_to_last_tab'))
  })

  test('a query that matches no label lists nothing', () => {
    assert.deepEqual(makeActionsSource({ keymap: DEFAULT_KEYMAP, onDispatch: recorder() })('qqqq'), [])
  })

  test('activating a row dispatches its action', async () => {
    const onDispatch = recorder()
    const [row] = makeActionsSource({ keymap: DEFAULT_KEYMAP, onDispatch })('reopen closed')
    await row.activate()
    assert.deepEqual(onDispatch.calls, [['restore_closed_tab']])
  })
})

describe('type to navigate', () => {
  const settings = {
    search_engine: 'duckduckgo',
    custom_search_engines: [{ name: 'Kagi', keyword: 'k', template: 'https://kagi.com/search?q={searchTerms}', is_default: false }],
    builtin_search_keywords: {},
  }

  test('an empty query offers nothing', () => {
    assert.deepEqual(makeNavigateSource({ onOpen: recorder(), settings })('  '), [])
  })

  test('text that is not an address offers a search through the active engine', () => {
    const [row] = makeNavigateSource({ onOpen: recorder(), settings })(' rust traits ')
    assert.equal(row.id, 'open:current')
    assert.equal(row.section, 'open')
    assert.equal(row.title, 'Search for "rust traits"')
    assert.equal(row.subtitle, 'via DuckDuckGo')
  })

  test('an address offers to go there', () => {
    const [row] = makeNavigateSource({ onOpen: recorder(), settings })('example.com')
    assert.equal(row.title, 'Go to https://example.com')
    assert.equal(row.subtitle, undefined)
  })

  test('keyword triggers apply, and activating opens the resolved URL', async () => {
    const onOpen = recorder()
    const [row] = makeNavigateSource({ onOpen, settings })('k cats')
    assert.equal(row.subtitle, 'via Kagi')
    await row.activate()
    assert.deepEqual(onOpen.calls, [['https://kagi.com/search?q=cats']])
  })
})

describe('bookmarks and history', () => {
  test('the history key drops the fragment', () => {
    assert.equal(historyKey('https://a.test/p?q=1#section'), 'https://a.test/p?q=1')
    assert.equal(historyKey('https://a.test/p'), 'https://a.test/p')
    assert.equal(historyKey('#only'), '')
  })

  test('an empty query lists no bookmarks or history', () => {
    const bookmarks = [{ id: 1, url: 'https://a.test/', title: 'A' }]
    const entries = [{ id: 1, url: 'https://a.test/', title: 'A', visit_count: 3 }]
    assert.deepEqual(makeBookmarksSource({ bookmarks, onOpen: recorder() })('  '), [])
    assert.deepEqual(makeHistorySource({ entries, onOpen: recorder() })(''), [])
  })

  test('a bookmark matches by title or URL and opens its URL', async () => {
    const onOpen = recorder()
    const bookmarks = [
      { id: 1, url: 'https://rust-lang.org/', title: 'Rust' },
      { id: 2, url: 'https://news.test/', title: 'News' },
      { id: 3, url: 'https://docs.test/rust', title: '  ' },
    ]
    const rows = makeBookmarksSource({ bookmarks, onOpen })(' rust ')
    assert.deepEqual(ids(rows).sort(), ['bookmark:1', 'bookmark:3'])
    const untitled = rows.find((it) => it.id === 'bookmark:3')
    assert.equal(untitled.title, 'https://docs.test/rust')
    assert.equal(untitled.section, 'bookmarks')
    await untitled.activate()
    assert.deepEqual(onOpen.calls, [['https://docs.test/rust']])
  })

  test('at most eight rows come from one page source, best first', () => {
    const bookmarks = Array.from({ length: 12 }, (_, i) => ({
      id: i,
      url: `https://site${i}.test/`,
      title: i === 11 ? 'match' : `${'x'.repeat(i)}match`,
    }))
    const rows = makeBookmarksSource({ bookmarks, onOpen: recorder() })('match')
    assert.equal(rows.length, 8)
    assert.equal(rows[0].id, 'bookmark:0')
    const scores = rows.map((it) => it.score)
    assert.deepEqual(scores, [...scores].sort((a, b) => b - a))
  })

  test('a page visited often outranks one seen once', () => {
    const entries = [
      { id: 1, url: 'https://a.test/rust', title: 'Rust', visit_count: 1 },
      { id: 2, url: 'https://b.test/rust', title: 'Rust', visit_count: 9 },
    ]
    assert.deepEqual(ids(makeHistorySource({ entries, onOpen: recorder() })('rust')), ['history:2', 'history:1'])
  })

  test('frequency lifts a page by at most half its match, and visits past ten stop counting', () => {
    const entry = (id, visits) => ({ id, url: 'https://x.test/', title: 'Rust', visit_count: visits })
    const base = fuzzyMatch('Rust', 'rust').score
    const rows = makeHistorySource({ entries: [entry(1, 0), entry(2, 10), entry(3, 1000)], onOpen: recorder() })('rust')
    const score = (id) => rows.find((it) => it.id === `history:${id}`).score
    assert.equal(score(1), base)
    assert.equal(score(2), base + Math.round(base / 2))
    assert.equal(score(3), score(2))
  })

  test('a history row opens its URL and marks what matched', async () => {
    const onOpen = recorder()
    const [row] = makeHistorySource({
      entries: [{ id: 7, url: 'https://a.test/', title: 'Alpha', visit_count: 1 }],
      onOpen,
    })('alp')
    assert.equal(row.section, 'history')
    assert.deepEqual(row.titleMatches, [0, 1, 2])
    assert.equal(row.subtitleMatches, undefined)
    await row.activate()
    assert.deepEqual(onOpen.calls, [['https://a.test/']])
  })
})

describe('recently closed tabs', () => {
  const entries = Array.from({ length: 7 }, (_, i) => ({
    id: 100 + i,
    url: `https://closed${i}.test/`,
    title: i === 1 ? ' ' : `Closed ${i}`,
  }))

  test('with no query the newest five are listed in order', () => {
    const rows = makeClosedTabsSource({ entries, onReopen: recorder() })('')
    assert.deepEqual(ids(rows), ['closed:100', 'closed:101', 'closed:102', 'closed:103', 'closed:104'])
    assert.deepEqual(
      rows.map((it) => it.score),
      [5, 4, 3, 2, 1],
    )
    assert.equal(rows[1].title, 'https://closed1.test/')
    assert.equal(rows[0].subtitle, 'https://closed0.test/')
  })

  test('a typed query searches the whole list', () => {
    const rows = makeClosedTabsSource({ entries, onReopen: recorder() })('closed 6')
    assert.equal(rows[0].id, 'closed:106')
    assert.equal(rows[0].section, 'closed')
  })

  test('a row reopens by its record id', async () => {
    const onReopen = recorder()
    const source = makeClosedTabsSource({ entries, onReopen })
    await source('')[2].activate()
    await source('closed 6')[0].activate()
    assert.deepEqual(onReopen.calls, [[102], [106]])
  })
})

describe('settings toggles', () => {
  const settings = {
    glass_enabled: true,
    sidebar_collapsible: false,
    url_bar_focus_glow: true,
    filter_list_auto_update: false,
  }

  test('every toggle is listed with its current state', () => {
    const rows = makeSettingsTogglesSource({ settings, onChange: recorder() })('')
    assert.deepEqual(
      Object.fromEntries(rows.map((it) => [it.id, it.accessory])),
      {
        'settings:glass_enabled': 'On',
        'settings:sidebar_collapsible': 'Off',
        'settings:url_bar_focus_glow': 'On',
        'settings:filter_list_auto_update': 'Off',
      },
    )
  })

  test('activating a toggle flips its setting', async () => {
    const onChange = recorder()
    const source = makeSettingsTogglesSource({ settings, onChange })
    await source('liquid glass')[0].activate()
    await source('collapsible')[0].activate()
    assert.deepEqual(onChange.calls, [[{ glass_enabled: false }], [{ sidebar_collapsible: true }]])
  })

  test('the verb "toggle" finds every toggle without marking the label', () => {
    const rows = makeSettingsTogglesSource({ settings, onChange: recorder() })('toggle')
    assert.equal(rows.length, 4)
    const glass = rows.find((it) => it.id === 'settings:glass_enabled')
    assert.equal(glass.titleMatches, undefined)
  })

  test('a query that matches no toggle lists nothing', () => {
    assert.deepEqual(makeSettingsTogglesSource({ settings, onChange: recorder() })('zzz'), [])
  })
})
