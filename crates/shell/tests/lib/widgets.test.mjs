// The sidebar widget catalog and custom widgets (`lib/widgets.ts`): rows,
// theme-aware panel URLs, custom URL checks, and drag placement.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, test } from 'node:test'

import {
  MAX_CUSTOM_WIDGET_LABEL_CHARS,
  MAX_CUSTOM_WIDGET_URL_BYTES,
  WIDGET_CATALOG,
  WIDGET_GROUPS,
  catalogEntry,
  customWidgetUrlProblem,
  dockSignature,
  effectiveWidgetUrl,
  makeCustomWidget,
  makeWidget,
  normalizeCustomWidgetUrl,
  placeWidget,
  widgetCategory,
} from '../../src/lib/widgets.ts'

const widget = (id, kind, extra = {}) => ({
  id,
  kind,
  label: id,
  url: catalogEntry(kind)?.url ?? 'https://example.com',
  icon: null,
  enabled: true,
  group: null,
  ...extra,
})

const NOT_AN_ADDRESS = 'Enter a web address, like notion.so or https://example.com.'

describe('catalog', () => {
  test('each built-in kind appears once, in a non-custom row, with a usable widget URL', () => {
    const kinds = WIDGET_CATALOG.map((e) => e.kind)
    assert.equal(new Set(kinds).size, kinds.length)
    for (const entry of WIDGET_CATALOG) {
      assert.ok(WIDGET_GROUPS.includes(entry.category), entry.kind)
      assert.notEqual(entry.category, 'custom', entry.kind)
      assert.equal(customWidgetUrlProblem(entry.url), null, entry.url)
      assert.ok(entry.url.startsWith('https://'), entry.url)
    }
  })

  test('a kind looks up its entry, and custom has none', () => {
    assert.equal(catalogEntry('claude').label, 'Claude')
    assert.equal(catalogEntry('claude').category, 'ai')
    assert.equal(catalogEntry('custom'), undefined)
  })

  test('the custom widget caps mirror core', () => {
    const core = readFileSync(new URL('../../../core/src/widget.rs', import.meta.url), 'utf8')
    const constant = (name) => Number(core.match(new RegExp(`const ${name}: usize = (\\d+);`))[1])
    assert.equal(MAX_CUSTOM_WIDGET_LABEL_CHARS, constant('MAX_WIDGET_LABEL_CHARS'))
    assert.equal(MAX_CUSTOM_WIDGET_URL_BYTES, constant('MAX_WIDGET_URL_BYTES'))
  })
})

describe('rows', () => {
  test('a built-in widget sits in its catalog row', () => {
    assert.equal(widgetCategory(widget('1', 'gmail')), 'productivity')
    assert.equal(widgetCategory(widget('2', 'discord')), 'social')
  })

  test('a custom widget sits in its chosen row, or the custom row without one', () => {
    assert.equal(widgetCategory(widget('1', 'custom', { group: 'ai' })), 'ai')
    assert.equal(widgetCategory(widget('2', 'custom')), 'custom')
  })

  test('a kind the catalog does not know falls back to the social row', () => {
    assert.equal(widgetCategory(widget('1', 'retired_kind')), 'social')
  })
})

describe('panel URL', () => {
  test('Instagram mirrors the chrome theme onto its theme parameter', () => {
    const ig = widget('1', 'instagram', { url: 'https://www.instagram.com/explore/?hl=en' })
    assert.equal(effectiveWidgetUrl(ig, true), 'https://www.instagram.com/explore/?hl=en&theme=dark')
    assert.equal(effectiveWidgetUrl(ig, false), 'https://www.instagram.com/explore/?hl=en')
    const darkSaved = { ...ig, url: 'https://www.instagram.com/?theme=dark' }
    assert.equal(effectiveWidgetUrl(darkSaved, false), 'https://www.instagram.com/')
    assert.equal(effectiveWidgetUrl(darkSaved, true), 'https://www.instagram.com/?theme=dark')
  })

  test('an unparseable Instagram URL loads as stored', () => {
    const ig = widget('1', 'instagram', { url: 'not a url' })
    assert.equal(effectiveWidgetUrl(ig, true), 'not a url')
  })

  test('the legacy Telegram clients are moved to the A client', () => {
    const legacy = [
      'https://web.telegram.org/',
      'https://web.telegram.org/k/',
      'https://web.telegram.org/k/#@chat',
    ]
    for (const url of legacy) {
      const tg = widget('1', 'telegram', { url })
      assert.equal(effectiveWidgetUrl(tg, false), 'https://web.telegram.org/a/')
    }
  })

  test('other Telegram addresses load as stored', () => {
    for (const url of ['https://web.telegram.org/a/#123', 'https://telegram.org/', 'not a url']) {
      assert.equal(effectiveWidgetUrl(widget('1', 'telegram', { url }), true), url)
    }
  })

  test('every other widget ignores the theme', () => {
    const claude = widget('1', 'claude')
    assert.equal(effectiveWidgetUrl(claude, true), 'https://claude.ai')
    assert.equal(effectiveWidgetUrl(claude, false), 'https://claude.ai')
  })
})

describe('new widgets', () => {
  test('a catalog widget starts enabled, in its own row, with a fresh id', () => {
    const entry = catalogEntry('notion')
    const a = makeWidget(entry)
    const b = makeWidget(entry)
    assert.deepEqual({ ...a, id: undefined }, {
      id: undefined,
      kind: 'notion',
      label: 'Notion',
      url: 'https://www.notion.so',
      icon: null,
      enabled: true,
      group: null,
    })
    assert.match(a.id, /^[0-9a-f-]{36}$/)
    assert.notEqual(a.id, b.id)
  })

  test('a custom widget trims its label, completes its URL, and keeps a non-custom row as its group', () => {
    const w = makeCustomWidget('  Docs  ', ' docs.example.com ', 'productivity')
    assert.equal(w.kind, 'custom')
    assert.equal(w.label, 'Docs')
    assert.equal(w.url, 'https://docs.example.com')
    assert.equal(w.group, 'productivity')
    assert.equal(w.enabled, true)
    assert.equal(makeCustomWidget('X', 'x.com', 'custom').group, null)
  })
})

describe('custom widget URLs', () => {
  test('a bare domain gets https and a URL with a scheme is kept', () => {
    assert.equal(normalizeCustomWidgetUrl('  notion.so '), 'https://notion.so')
    assert.equal(normalizeCustomWidgetUrl('http://intranet.local'), 'http://intranet.local')
    assert.equal(normalizeCustomWidgetUrl('   '), '')
  })

  test('an empty field has no problem yet', () => {
    assert.equal(customWidgetUrlProblem(''), null)
    assert.equal(customWidgetUrlProblem('  '), null)
  })

  test('web addresses with or without a scheme are accepted', () => {
    assert.equal(customWidgetUrlProblem('notion.so'), null)
    assert.equal(customWidgetUrlProblem('http://localhost:3000/app'), null)
  })

  test('text that is not an address is refused', () => {
    assert.equal(customWidgetUrlProblem('two words'), NOT_AN_ADDRESS)
    assert.equal(customWidgetUrlProblem('javascript:alert(1)'), NOT_AN_ADDRESS)
  })

  test('schemes other than http and https are refused', () => {
    const onlyWeb = 'Only http:// and https:// sites can be widgets.'
    assert.equal(customWidgetUrlProblem('ftp://files.example.com'), onlyWeb)
    assert.equal(customWidgetUrlProblem('file:///etc/hosts'), onlyWeb)
  })

  test('the length cap counts bytes, up to and including the limit', () => {
    const base = 'https://example.com/'
    const atLimit = base + 'a'.repeat(MAX_CUSTOM_WIDGET_URL_BYTES - base.length)
    assert.equal(customWidgetUrlProblem(atLimit), null)
    assert.equal(customWidgetUrlProblem(atLimit + 'a'), 'That web address is too long for a widget.')
    // Two-byte characters cross the cap at half the character count.
    const wide = base + 'é'.repeat(MAX_CUSTOM_WIDGET_URL_BYTES / 2)
    assert.equal(customWidgetUrlProblem(wide), 'That web address is too long for a widget.')
  })
})

describe('placing a dragged widget', () => {
  const dock = () => [
    widget('wa', 'whats_app'),
    widget('gm', 'gmail'),
    widget('cl', 'claude'),
    widget('dc', 'discord'),
    widget('c1', 'custom'),
    widget('c2', 'custom', { group: 'ai' }),
  ]

  test('the stored list comes back in canonical row order', () => {
    const next = placeWidget(dock(), 'dc', 'social', 0)
    assert.deepEqual(
      next.map((w) => w.id),
      ['dc', 'wa', 'cl', 'c2', 'gm', 'c1'],
    )
    assert.equal(dockSignature(next), 'dc,wa|cl,c2|gm|c1')
  })

  test('an index counts the row without the dragged widget and is clamped to it', () => {
    assert.equal(dockSignature(placeWidget(dock(), 'wa', 'social', 1)), 'dc,wa|cl,c2|gm|c1')
    assert.equal(dockSignature(placeWidget(dock(), 'wa', 'social', 99)), 'dc,wa|cl,c2|gm|c1')
    assert.equal(dockSignature(placeWidget(dock(), 'dc', 'social', -3)), 'dc,wa|cl,c2|gm|c1')
  })

  test('a built-in widget cannot leave its row', () => {
    assert.equal(placeWidget(dock(), 'gm', 'ai', 0), null)
  })

  test('an unknown id places nothing', () => {
    assert.equal(placeWidget(dock(), 'nope', 'social', 0), null)
  })

  test('a custom widget moved to another row stores that row as its group', () => {
    const next = placeWidget(dock(), 'c1', 'productivity', 0)
    assert.equal(dockSignature(next), 'wa,dc|cl,c2|c1,gm|')
    assert.equal(next.find((w) => w.id === 'c1').group, 'productivity')
  })

  test('a custom widget moved back to the custom row clears its group', () => {
    const next = placeWidget(dock(), 'c2', 'custom', 1)
    assert.equal(dockSignature(next), 'wa,dc|cl|gm|c1,c2')
    assert.equal(next.find((w) => w.id === 'c2').group, null)
  })

  test('the stored list is left untouched', () => {
    const before = dock()
    placeWidget(before, 'c1', 'ai', 0)
    assert.deepEqual(before, dock())
  })

  test('the dock signature keeps empty rows so a move between rows always changes it', () => {
    assert.equal(dockSignature([]), '|||')
    const inCustom = dockSignature([widget('c', 'custom')])
    const inAi = dockSignature([widget('c', 'custom', { group: 'ai' })])
    assert.notEqual(inCustom, inAi)
  })
})
