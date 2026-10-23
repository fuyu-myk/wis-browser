// How the top tab strip shares its width (`features/tabs/lib/tabStripLayout.ts`): name chip
// widths, the active/inactive split, the fixed reservations, and list moves.

import assert from 'node:assert/strict'
import { beforeEach, describe, test } from 'node:test'

import {
  AUDIO_BTN_W,
  COMPACT_MINIMUM_CURRENT_TAB_SIZE,
  COMPACT_MINIMUM_TAB_SIZE,
  COMPACT_TAB_CONTENT_GAP,
  GAP_PX,
  ISLAND_CIRCLE_W,
  ISLAND_NAME_MAX,
  ISLAND_NAME_MAX_COMPACT,
  ISLAND_NAME_W,
  ISLAND_NAME_W_COMPACT,
  ISLAND_PILL_PAD,
  MINIMUM_CURRENT_TAB_SIZE,
  MINIMUM_TAB_SIZE,
  PIN_CIRCLE_W,
  TAB_CONTENT_GAP,
  chipTextWidth,
  chipWidthFor,
  computeSizing,
  computeStripSizing,
  moveInList,
} from '../../src/features/tabs/lib/tabStripLayout.ts'

// Text measures 10px per character plus a fraction, so rounding up shows.
let canvasWorks = true
const measured = []
const fakeContext = {
  font: '',
  measureText(text) {
    measured.push(text)
    return { width: text.length * 10 + 0.4 }
  },
}
globalThis.document = {
  createElement: (tag) => ({
    getContext: (kind) => (canvasWorks && tag === 'canvas' && kind === '2d' ? fakeContext : null),
  }),
}

beforeEach(() => {
  measured.length = 0
})

const tab = (id) => ({ id })
const folder = (id, extra = {}) => ({ id, name: 'Work', icon: null, collapsed: false, ...extra })

describe('chip measurement', () => {
  // The module keeps the first context it gets, so this has to measure first.
  test('without a 2d canvas context a name measures zero, and a later name tries again', () => {
    canvasWorks = false
    try {
      assert.equal(chipTextWidth('Anything'), 0)
      assert.equal(chipWidthFor(folder('f', { name: 'Anything' }), false), ISLAND_NAME_W)
    } finally {
      canvasWorks = true
    }
    assert.equal(chipTextWidth('Something'), 91)
  })

  test('text is measured once in the chip font and rounded up', () => {
    assert.equal(chipTextWidth('Reading'), 71)
    assert.equal(chipTextWidth('Reading'), 71)
    assert.deepEqual(measured, ['Reading'])
    assert.match(fakeContext.font, /^600 12px Geist/)
  })

  test('a short name holds the chip at its minimum width', () => {
    assert.equal(chipWidthFor(folder('f', { name: 'A' }), false), ISLAND_NAME_W)
    assert.equal(chipWidthFor(folder('f', { name: 'A' }), true), ISLAND_NAME_W_COMPACT)
  })

  test('a mid-length name fits the chip to its text plus padding and chevron', () => {
    // 5 + 15 + 3 + 9 of fixed parts around 81px of text.
    assert.equal(chipWidthFor(folder('f', { name: 'Research' }), false), 113)
    assert.equal(chipWidthFor(folder('f', { name: 'Research' }), true), 113)
  })

  test('an icon adds its glyph and label gap', () => {
    assert.equal(chipWidthFor(folder('f', { name: 'Research', icon: 'emoji:x' }), false), 128)
  })

  test('a long name is capped and truncates', () => {
    const long = folder('f', { name: 'A very long folder name' })
    assert.equal(chipWidthFor(long, false), ISLAND_NAME_MAX)
    assert.equal(chipWidthFor(long, true), ISLAND_NAME_MAX_COMPACT)
  })
})

describe('sharing the flexible width', () => {
  const MIN_A = MINIMUM_CURRENT_TAB_SIZE
  const MIN_I = MINIMUM_TAB_SIZE

  test('no tabs leaves the floors in place without scrolling', () => {
    assert.deepEqual(computeSizing(0, 500, MIN_A, MIN_I), {
      activeSize: MIN_A,
      inactiveSize: MIN_I,
      scrolling: false,
    })
  })

  test('tabs with no room left scroll at the floors', () => {
    assert.deepEqual(computeSizing(3, 0, MIN_A, MIN_I), {
      activeSize: MIN_A,
      inactiveSize: MIN_I,
      scrolling: true,
    })
  })

  test('every tab gets an equal share while that share reaches the active floor', () => {
    assert.deepEqual(computeSizing(3, 900, MIN_A, MIN_I), {
      activeSize: 300,
      inactiveSize: 300,
      scrolling: false,
    })
    assert.deepEqual(computeSizing(2, 2 * MIN_A, MIN_A, MIN_I), {
      activeSize: MIN_A,
      inactiveSize: MIN_A,
      scrolling: false,
    })
  })

  test('a lone tab narrower than the active floor keeps the floor and scrolls', () => {
    assert.deepEqual(computeSizing(1, 200, MIN_A, MIN_I), {
      activeSize: MIN_A,
      inactiveSize: MIN_I,
      scrolling: true,
    })
  })

  test('when space runs short the active tab keeps its floor and the rest split the remainder', () => {
    assert.deepEqual(computeSizing(5, 600, MIN_A, MIN_I), {
      activeSize: MIN_A,
      inactiveSize: 85,
      scrolling: false,
    })
  })

  test('inactive tabs exactly at their floor still fit, one pixel less scrolls', () => {
    const exact = MIN_A + 4 * MIN_I
    assert.deepEqual(computeSizing(5, exact, MIN_A, MIN_I), {
      activeSize: MIN_A,
      inactiveSize: MIN_I,
      scrolling: false,
    })
    assert.deepEqual(computeSizing(5, exact - 1, MIN_A, MIN_I), {
      activeSize: MIN_A,
      inactiveSize: MIN_I,
      scrolling: true,
    })
  })
})

describe('strip reservations', () => {
  // Mirrors how HorizontalTabs builds the inputs: collapsed and closing
  // islands count as circles and their members are not sized.
  function strip({
    pinned = 0,
    loose = [],
    groups = [],
    closingIslandId = null,
    stripWidth = 1000,
    compact = false,
    active = null,
    audio = [],
  } = {}) {
    const sizableTabs = [...loose]
    let collapsedCount = 0
    for (const g of groups) {
      if (g.folder.collapsed || g.folder.id === closingIslandId) collapsedCount += 1
      else sizableTabs.push(...g.members)
    }
    return computeStripSizing({
      sizableTabs,
      looseTabs: loose,
      pinnedCount: pinned,
      folderGroups: groups,
      collapsedCount,
      closingIslandId,
      stripWidth,
      compact,
      activeTabId: active,
      minActive: compact ? COMPACT_MINIMUM_CURRENT_TAB_SIZE : MINIMUM_CURRENT_TAB_SIZE,
      minInactive: compact ? COMPACT_MINIMUM_TAB_SIZE : MINIMUM_TAB_SIZE,
      isAudio: (t) => audio.includes(t.id),
    })
  }

  test('pinned circles and the gaps between top-level items come off the top', () => {
    const r = strip({ pinned: 2, loose: [tab('a'), tab('b'), tab('c')], active: 'b' })
    const available = 1000 - 2 * PIN_CIRCLE_W - 4 * GAP_PX
    assert.equal(r.sizing.activeSize, available / 3)
    assert.equal(r.sizing.inactiveSize, available / 3)
    assert.equal(r.isMany, false)
    assert.equal(r.inactiveCompact, false)
    assert.equal(r.activeVisible, true)
  })

  test('each audio tab reserves its speaker button and gap', () => {
    const loose = [tab('a'), tab('b'), tab('c')]
    const r = strip({ loose, audio: ['a'] })
    assert.equal(r.audioExtra, AUDIO_BTN_W + TAB_CONTENT_GAP)
    assert.equal(r.sizing.activeSize, (1000 - 2 * GAP_PX - r.audioExtra) / 3)
    assert.equal(strip({ loose, compact: true }).audioExtra, AUDIO_BTN_W + COMPACT_TAB_CONTENT_GAP)
  })

  test('an expanded island reserves its chip and pill padding, and a gap per member', () => {
    const f = folder('f', { name: 'Research' })
    const members = [tab('m1'), tab('m2')]
    const r = strip({ loose: [tab('a')], groups: [{ folder: f, members }], stripWidth: 1200 })
    const chip = chipWidthFor(f, false) + 2 * ISLAND_PILL_PAD
    const available = 1200 - chip - 1 * GAP_PX - 2 * GAP_PX
    assert.equal(r.sizing.activeSize, available / 3)
  })

  test('collapsed and closing islands reserve only their circle', () => {
    const collapsed = { folder: folder('c', { collapsed: true }), members: [tab('x')] }
    const closing = { folder: folder('k'), members: [tab('y')] }
    const r = strip({ loose: [tab('a')], groups: [collapsed, closing], closingIslandId: 'k' })
    const available = 1000 - 2 * ISLAND_CIRCLE_W - 2 * GAP_PX
    assert.equal(r.sizing.activeSize, available)
  })

  test('tight space makes the strip many-sized and compacts inactive tabs below the threshold', () => {
    const loose = ['a', 'b', 'c', 'd', 'e', 'f'].map(tab)
    const roomy = strip({ loose, stripWidth: 260 + 5 * 140 + 5 * GAP_PX, active: 'a' })
    assert.equal(roomy.isMany, true)
    assert.equal(roomy.sizing.inactiveSize, 140)
    assert.equal(roomy.inactiveCompact, false)

    const tight = strip({ loose, stripWidth: 260 + 5 * 120 + 5 * GAP_PX, active: 'a' })
    assert.equal(tight.sizing.inactiveSize, 120)
    assert.equal(tight.inactiveCompact, true)
  })

  test('the compact strip compacts inactive tabs only below its own lower threshold', () => {
    const loose = ['a', 'b', 'c', 'd', 'e', 'f'].map(tab)
    const minA = COMPACT_MINIMUM_CURRENT_TAB_SIZE
    const at100 = strip({ loose, compact: true, stripWidth: minA + 5 * 100 + 5 * GAP_PX })
    assert.equal(at100.sizing.inactiveSize, 100)
    assert.equal(at100.inactiveCompact, false)
    const at80 = strip({ loose, compact: true, stripWidth: minA + 5 * 80 + 5 * GAP_PX })
    assert.equal(at80.inactiveCompact, true)
  })

  test('a strip narrower than its fixed parts scrolls rather than going negative', () => {
    const r = strip({ pinned: 10, loose: [tab('a'), tab('b')], stripWidth: 100 })
    assert.equal(r.sizing.scrolling, true)
    assert.equal(r.sizing.activeSize, MINIMUM_CURRENT_TAB_SIZE)
    assert.equal(r.sizing.inactiveSize, MINIMUM_TAB_SIZE)
  })

  test('the active tab is not visible when it is pinned, filed in a collapsed island, or absent', () => {
    const collapsed = { folder: folder('c', { collapsed: true }), members: [tab('x')] }
    assert.equal(strip({ loose: [tab('a')], groups: [collapsed], active: 'x' }).activeVisible, false)
    assert.equal(strip({ pinned: 1, loose: [tab('a')], active: 'p' }).activeVisible, false)
    assert.equal(strip({ loose: [tab('a')], active: null }).activeVisible, false)
  })
})

describe('moving within a list', () => {
  test('moving forward lands before the insertion point', () => {
    assert.deepEqual(moveInList(['a', 'b', 'c', 'd'], 'a', 3), ['b', 'c', 'a', 'd'])
    assert.deepEqual(moveInList(['a', 'b', 'c', 'd'], 'a', 4), ['b', 'c', 'd', 'a'])
  })

  test('moving backward lands at the insertion point', () => {
    assert.deepEqual(moveInList(['a', 'b', 'c', 'd'], 'd', 1), ['a', 'd', 'b', 'c'])
    assert.deepEqual(moveInList(['a', 'b', 'c', 'd'], 'c', 0), ['c', 'a', 'b', 'd'])
  })

  test('dropping onto either side of itself changes nothing', () => {
    assert.equal(moveInList(['a', 'b', 'c'], 'b', 1), null)
    assert.equal(moveInList(['a', 'b', 'c'], 'b', 2), null)
  })

  test('an id not in the list changes nothing', () => {
    assert.equal(moveInList(['a', 'b'], 'z', 0), null)
  })

  test('the input order is left untouched', () => {
    const order = ['a', 'b', 'c']
    moveInList(order, 'a', 3)
    assert.deepEqual(order, ['a', 'b', 'c'])
  })
})
