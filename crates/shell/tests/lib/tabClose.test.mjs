// Closing a tab with its exit animation, and hearing about closes from any
// webview (`features/tabs/lib/tabClose.ts`).

import assert from 'node:assert/strict'
import { beforeEach, describe, test } from 'node:test'

import { TAB_CLOSING_EVENT, closeTabAnimated, onTabClosing, withClosingGhosts } from '../../src/features/tabs/lib/tabClose.ts'
import { noteActiveTab, pruneTabMru, recentTabs } from '../../src/features/tabs/lib/tabMru.ts'
import { installTauri, settle } from '../support/l3-harness.mjs'

let ipc
beforeEach(() => {
  ipc = installTauri()
  pruneTabMru(() => false)
})

describe('closing a tab', () => {
  test('announces the id in this webview before any IPC is sent', () => {
    const seen = []
    window.addEventListener(TAB_CLOSING_EVENT, (e) => seen.push([e.detail, ipc.calls.length]))
    closeTabAnimated('b')
    assert.deepEqual(seen, [['b', 0]])
  })

  test('broadcasts the id to every other webview', async () => {
    closeTabAnimated('b')
    await settle()
    assert.deepEqual(ipc.argsOf('plugin:event|emit'), [{ event: TAB_CLOSING_EVENT, payload: 'b' }])
  })

  test('the active tab returns to the one used before it, with the rest of the history', async () => {
    for (const id of ['a', 'c', 'b']) noteActiveTab(id, 'ws')
    closeTabAnimated('b')
    await settle()
    assert.deepEqual(ipc.argsOf('close_tab'), [{ id: 'b', returnTo: 'c', recent: ['c', 'a'] }])
  })

  test('a background tab returns nowhere and leaves the history', async () => {
    for (const id of ['a', 'c', 'b']) noteActiveTab(id, 'ws')
    closeTabAnimated('a')
    await settle()
    assert.deepEqual(ipc.argsOf('close_tab'), [{ id: 'a', returnTo: null, recent: ['b', 'c'] }])
    assert.deepEqual(recentTabs(), ['b', 'c'])
  })

  test('IPC failures are swallowed once the close is announced', async () => {
    const fail = () => {
      throw new Error('ipc down')
    }
    ipc.respond('plugin:event|emit', fail)
    ipc.respond('close_tab', fail)
    const seen = []
    window.addEventListener(TAB_CLOSING_EVENT, (e) => seen.push(e.detail))
    assert.doesNotThrow(() => closeTabAnimated('b'))
    await settle()
    assert.deepEqual(seen, ['b'])
  })
})

describe('subscribing to closes', () => {
  test('a close in this webview arrives on both channels, so the callback runs twice', async () => {
    const seen = []
    onTabClosing((id) => seen.push(id))
    await settle()
    closeTabAnimated('b')
    await settle()
    assert.deepEqual(seen, ['b', 'b'])
  })

  test('a close announced by another webview arrives over Tauri', async () => {
    const seen = []
    onTabClosing((id) => seen.push(id))
    await settle()
    ipc.deliver(TAB_CLOSING_EVENT, 'x')
    assert.deepEqual(seen, ['x'])
  })

  test('a DOM close without an id is ignored', () => {
    const seen = []
    onTabClosing((id) => seen.push(id))
    window.dispatchEvent(new CustomEvent(TAB_CLOSING_EVENT))
    assert.deepEqual(seen, [])
  })

  test('unsubscribing silences both channels and releases the Tauri listener', async () => {
    const seen = []
    const stop = onTabClosing((id) => seen.push(id))
    await settle()
    stop()
    await settle()
    window.dispatchEvent(new CustomEvent(TAB_CLOSING_EVENT, { detail: 'b' }))
    ipc.deliver(TAB_CLOSING_EVENT, 'b')
    assert.deepEqual(seen, [])
    assert.equal(ipc.listenerCount(TAB_CLOSING_EVENT), 0)
  })

  test('unsubscribing before the Tauri listener is in place releases it once it lands', async () => {
    let land
    ipc.respond('plugin:event|listen', () => new Promise((resolve) => (land = resolve)))
    const stop = onTabClosing(() => {})
    stop()
    land(42)
    await settle()
    assert.deepEqual(ipc.argsOf('plugin:event|unlisten'), [{ event: TAB_CLOSING_EVENT, eventId: 42 }])
  })
})

describe('withClosingGhosts', () => {
  const tab = (id) => ({ id })
  const ids = (list) => list.map((t) => t.id)

  test('a closing tab the backend already removed stays in its old slot', () => {
    const prev = [tab('a'), tab('b'), tab('c')]
    const now = [tab('a'), tab('c')]
    assert.deepEqual(ids(withClosingGhosts(now, prev, new Set(['b']))), ['a', 'b', 'c'])
  })

  test('nothing closing, or nothing removed yet, leaves the list as it is', () => {
    const now = [tab('a'), tab('b')]
    assert.equal(withClosingGhosts(now, now, new Set()), now)
    assert.equal(withClosingGhosts(now, now, new Set(['b'])), now)
  })

  test('a ghost past the end of the shorter list goes last', () => {
    const prev = [tab('a'), tab('b'), tab('c')]
    const now = [tab('a')]
    assert.deepEqual(ids(withClosingGhosts(now, prev, new Set(['c']))), ['a', 'c'])
  })
})
