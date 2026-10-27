// The address bar dropdown's row contract and sizing (`features/urlbar/urlbarSuggest.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  estimateHeight,
  isRemovable,
  SECTION_RECENT_SEARCHES,
  SECTION_RECOMMENDATIONS,
  SECTION_TOP,
  spokenRow,
} from '../../src/features/urlbar/urlbarSuggest.ts'

const site = (section = SECTION_TOP) => ({ kind: 'site', section, id: 1, url: 'https://a.test/', title: 'A', openTabId: null })
const searchRow = (section = SECTION_RECENT_SEARCHES) => ({ kind: 'search', section, query: 'cats' })
const typedQuery = () => ({ kind: 'searchQuery', section: SECTION_TOP, query: 'cats', engineLabel: 'DuckDuckGo' })

// Mirrors the metrics in `window.css`: 6px padding top and bottom, 40px rows,
// 26px section headers, 4px between sections.
const PAD = 12
const ROW = 40
const HEAD = 26
const GAP = 4

test('history-backed rows can be forgotten; the typed search row cannot', () => {
  assert.equal(isRemovable(site()), true)
  assert.equal(isRemovable(searchRow()), true)
  assert.equal(isRemovable(typedQuery()), false)
})

test('an empty list takes no height', () => {
  assert.equal(estimateHeight([]), 0)
})

test('the untitled top section has no header', () => {
  assert.equal(estimateHeight([typedQuery(), site()]), PAD + 2 * ROW)
})

test('each titled section adds a header, and each section after the first a gap', () => {
  const rows = [typedQuery(), site(SECTION_RECOMMENDATIONS), site(SECTION_RECOMMENDATIONS), searchRow()]
  assert.equal(estimateHeight(rows), PAD + 4 * ROW + 2 * HEAD + 2 * GAP)
})

test('a list that starts with a titled section has its header counted', () => {
  assert.equal(estimateHeight([searchRow()]), PAD + ROW + HEAD)
})

test('a section that resumes after another counts as a new group', () => {
  const rows = [site(SECTION_RECOMMENDATIONS), searchRow(), site(SECTION_RECOMMENDATIONS)]
  assert.equal(estimateHeight(rows), PAD + 3 * ROW + 3 * HEAD + 2 * GAP)
})

test('the dropdown height is capped', () => {
  const rows = Array.from({ length: 30 }, () => site())
  assert.equal(estimateHeight(rows), 460)
})

test('the highlighted suggestion is read with its kind and place in the list', () => {
  const site = { kind: 'site', section: 'Top', id: 1, url: 'https://www.example.com/a', title: 'Example', openTabId: null }
  assert.equal(spokenRow(site, 0, 3), 'Example, example.com, 1 of 3')
  assert.equal(spokenRow({ ...site, title: '' }, 1, 3), 'example.com, 2 of 3')
  assert.equal(spokenRow({ ...site, openTabId: 'tab' }, 0, 1), 'Example, example.com, switch to tab, 1 of 1')
  assert.equal(spokenRow({ kind: 'search', section: 'Recent', query: 'cats' }, 2, 3), 'cats, search, 3 of 3')
  assert.equal(
    spokenRow({ kind: 'searchQuery', section: 'Search', query: 'cats', engineLabel: 'Kagi' }, 0, 2),
    'cats, search with Kagi, 1 of 2',
  )
})
