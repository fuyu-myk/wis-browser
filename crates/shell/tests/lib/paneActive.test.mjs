// Which tab the chrome predicts becomes active after a close
// (`lib/paneActive.ts`), mirroring `Workspace::with_tab_removed` and
// `with_pane_after_removed` in core.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { predictActiveAfterClose } from '../../src/lib/paneActive.ts'

const leaf = (tab) => ({ kind: 'leaf', tab })
const split = (first, second) => ({ kind: 'split', dir: 'row', ratio: 500, first, second })

describe('without a split', () => {
  test('closing an inactive tab keeps the active one', () => {
    assert.equal(predictActiveAfterClose('b', 'c', ['a', 'b', 'c'], null), 'b')
  })

  test('closing the active tab hands off to the tab before it', () => {
    assert.equal(predictActiveAfterClose('c', 'c', ['a', 'b', 'c', 'd'], null), 'b')
  })

  test('closing the active first tab hands off to the new first', () => {
    assert.equal(predictActiveAfterClose('a', 'a', ['a', 'b', 'c'], null), 'b')
  })

  test('closing the last remaining tab leaves nothing active', () => {
    assert.equal(predictActiveAfterClose('a', 'a', ['a'], null), null)
  })

  test.todo(
    'an active tab missing from the order hands off to the last tab, as core does (bug: picks the first tab instead; unreachable from HorizontalTabs, which passes the full tab order)',
  )
})

describe('with a split', () => {
  test('closing a tab outside the split falls back to the order', () => {
    const tree = split(leaf('a'), leaf('b'))
    assert.equal(predictActiveAfterClose('d', 'd', ['a', 'b', 'c', 'd'], tree), 'c')
  })

  test('closing one of two panes makes the survivor active, whatever the order says', () => {
    const tree = split(leaf('a'), leaf('c'))
    assert.equal(predictActiveAfterClose('c', 'c', ['a', 'b', 'c'], tree), 'a')
    assert.equal(predictActiveAfterClose('a', 'a', ['a', 'b', 'c'], tree), 'c')
  })

  test('a split collapsing to one pane makes that pane active even when another tab was', () => {
    const tree = split(leaf('a'), leaf('b'))
    assert.equal(predictActiveAfterClose('c', 'b', ['a', 'b', 'c'], tree), 'a')
  })

  test('an active pane that survives a close stays active', () => {
    const tree = split(leaf('a'), split(leaf('b'), leaf('c')))
    assert.equal(predictActiveAfterClose('c', 'b', ['a', 'b', 'c'], tree), 'c')
  })

  test('the tab before a closed active pane stays active when it is still a pane', () => {
    const tree = split(leaf('a'), split(leaf('b'), leaf('c')))
    assert.equal(predictActiveAfterClose('b', 'b', ['a', 'b', 'c'], tree), 'a')
  })

  test('otherwise the first surviving pane becomes active', () => {
    const tree = split(split(leaf('c'), leaf('b')), leaf('a'))
    assert.equal(predictActiveAfterClose('b', 'b', ['x', 'b', 'c', 'a'], tree), 'c')
  })

  test('closing the only pane of a single-leaf layout falls back to the order', () => {
    assert.equal(predictActiveAfterClose('b', 'b', ['a', 'b'], leaf('b')), 'a')
  })

  test('the closed pane collapses its split so a deeper sibling takes its place', () => {
    // After closing c, the tree is split(a, b); d was before c in order but
    // is not a pane, so the first surviving pane wins.
    const tree = split(leaf('a'), split(leaf('c'), leaf('b')))
    assert.equal(predictActiveAfterClose('c', 'c', ['a', 'd', 'c', 'b'], tree), 'a')
    assert.equal(predictActiveAfterClose('c', 'c', ['a', 'b', 'c'], tree), 'b')
  })
})
