// Which tab the chrome predicts becomes active after a close
// (`lib/paneActive.ts`), mirroring core's `close_tab`: the return to the
// previous tab, `with_tab_closed_keeping_split`, `with_tab_removed` and
// `with_pane_after_removed`.

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

  test('an active tab missing from the order hands off to the last tab, as core does', () => {
    assert.equal(predictActiveAfterClose('z', 'z', ['a', 'b', 'c'], null), 'c')
  })

  test('closing the active tab returns to the tab used before it', () => {
    assert.equal(predictActiveAfterClose('c', 'c', ['a', 'b', 'c'], null, ['a', 'b']), 'a')
  })

  test('a previous tab from another workspace is not returned to', () => {
    assert.equal(predictActiveAfterClose('c', 'c', ['a', 'b', 'c'], null, ['elsewhere', 'a']), 'b')
  })
})

describe('with a split', () => {
  test('closing a tab outside the split falls back to the order', () => {
    const tree = split(leaf('a'), leaf('b'))
    assert.equal(predictActiveAfterClose('d', 'd', ['a', 'b', 'c', 'd'], tree), 'c')
  })

  test('closing a pane refills it with the most recent tab not in a pane, which takes focus', () => {
    const tree = split(leaf('a'), leaf('c'))
    assert.equal(predictActiveAfterClose('c', 'c', ['a', 'b', 'd', 'c'], tree, ['d', 'a', 'b']), 'd')
  })

  test('with no history the refill is the nearest tab in order', () => {
    const tree = split(leaf('a'), leaf('c'))
    assert.equal(predictActiveAfterClose('c', 'c', ['a', 'b', 'c'], tree), 'b')
  })

  test('refilling an unfocused pane leaves focus where it is', () => {
    const tree = split(leaf('a'), leaf('b'))
    assert.equal(predictActiveAfterClose('a', 'b', ['a', 'b', 'c'], tree), 'a')
  })

  test('with every other tab already a pane, closing one of two makes the survivor active', () => {
    const tree = split(leaf('a'), leaf('c'))
    assert.equal(predictActiveAfterClose('c', 'c', ['a', 'c'], tree), 'a')
    assert.equal(predictActiveAfterClose('a', 'a', ['a', 'c'], tree), 'c')
  })

  test('an active pane that survives a close stays active', () => {
    const tree = split(leaf('a'), split(leaf('b'), leaf('c')))
    assert.equal(predictActiveAfterClose('c', 'b', ['a', 'b', 'c'], tree), 'c')
  })

  test('the tab before a closed active pane stays active when it is still a pane', () => {
    const tree = split(leaf('a'), split(leaf('b'), leaf('c')))
    assert.equal(predictActiveAfterClose('b', 'b', ['a', 'b', 'c'], tree), 'a')
  })

  test('closing the only pane of a single-leaf layout falls back to the order', () => {
    assert.equal(predictActiveAfterClose('b', 'b', ['a', 'b'], leaf('b')), 'a')
  })

  test('the closed pane collapses its split so a deeper sibling takes its place', () => {
    const tree = split(leaf('a'), split(leaf('c'), leaf('b')))
    assert.equal(predictActiveAfterClose('c', 'c', ['a', 'b', 'c'], tree), 'b')
  })
})
