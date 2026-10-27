// Drag-reorder choreography (`features/tabs/lib/tabMotion.ts`): parting offsets, the
// optimistic cache patches, and the landing and workspace-slide animations.

import assert from 'node:assert/strict'
import { beforeEach, describe, test } from 'node:test'

import {
  glideIntoPlace,
  partOffset,
  patchFolderOrder,
  patchTabFolder,
  patchTabOrder,
  runPendingGlide,
  workspaceSlideIn,
} from '../../src/features/tabs/lib/tabMotion.ts'
import { moveInList } from '../../src/features/tabs/lib/tabStripLayout.ts'

const bodyClasses = new Set()
globalThis.document = { body: { classList: { contains: (c) => bodyClasses.has(c) } } }

beforeEach(() => bodyClasses.clear())

function fakeElement(rect = { left: 0, top: 0 }) {
  const animations = []
  let measured = 0
  return {
    animations,
    get measured() {
      return measured
    },
    getBoundingClientRect() {
      measured += 1
      return rect
    },
    animate(frames, options) {
      animations.push({ frames, options })
    },
  }
}

function fakeQueryClient(data) {
  const keys = []
  return {
    keys,
    data,
    setQueryData(key, update) {
      keys.push(key)
      this.data = update(this.data)
    },
  }
}

describe('parting while a drag is in flight', () => {
  test('nothing parts before a drop slot is known, and the dragged item never shifts', () => {
    assert.equal(partOffset(2, 0, -1), 0)
    assert.equal(partOffset(1, 1, 3), 0)
  })

  test('dragging forward pulls the items it passes back one slot', () => {
    const shifts = [0, 1, 2, 3, 4, 5].map((i) => partOffset(i, 1, 4))
    assert.deepEqual(shifts, [0, 0, -1, -1, 0, 0])
  })

  test('dragging backward pushes the items it passes forward one slot', () => {
    const shifts = [0, 1, 2, 3, 4, 5].map((i) => partOffset(i, 4, 1))
    assert.deepEqual(shifts, [0, 1, 1, 1, 0, 0])
  })

  test('an item entering from outside parts everything from its slot onward by default', () => {
    const shifts = [0, 1, 2, 3].map((i) => partOffset(i, -1, 2))
    assert.deepEqual(shifts, [0, 0, 1, 1])
  })

  test('an item entering from outside can part the items before its slot backward instead', () => {
    const shifts = [0, 1, 2, 3].map((i) => partOffset(i, -1, 2, -1))
    assert.deepEqual(shifts, [-1, -1, 0, 0])
  })

  test('the parted preview matches the order the drop commits', () => {
    const order = ['a', 'b', 'c', 'd', 'e']
    for (let from = 0; from < order.length; from++) {
      for (let to = 0; to <= order.length; to++) {
        const preview = []
        order.forEach((id, i) => {
          const at = i === from ? (to > from ? to - 1 : to) : i + partOffset(i, from, to)
          preview[at] = id
        })
        const committed = moveInList(order, order[from], to) ?? order
        assert.deepEqual(preview, committed, `from ${from} to ${to}`)
      }
    }
  })
})

describe('landing a drop', () => {
  const ghost = { left: 120, top: 40 }

  test('glides from the ghost into the slot with a settle pop', () => {
    const el = fakeElement({ left: 20, top: 30 })
    glideIntoPlace(el, ghost)
    assert.equal(el.animations.length, 1)
    const { frames, options } = el.animations[0]
    assert.deepEqual(frames, [
      { transform: 'translate(100px, 10px) scale(1)' },
      { transform: 'translate(0, 0) scale(1.04)', offset: 0.75 },
      { transform: 'translate(0, 0) scale(1)' },
    ])
    assert.equal(options.duration, 280)
  })

  test('reduced motion lands in place without moving', () => {
    bodyClasses.add('motion-reduced')
    const el = fakeElement({ left: 20, top: 30 })
    glideIntoPlace(el, ghost)
    assert.equal(el.animations.length, 0)
    assert.equal(el.measured, 0)
  })

  test('motion off does not animate or even measure', () => {
    bodyClasses.add('motion-off')
    const el = fakeElement({ left: 20, top: 30 })
    glideIntoPlace(el, ghost)
    assert.equal(el.animations.length, 0)
    assert.equal(el.measured, 0)
  })

  test('a landing under a pixel away is not animated', () => {
    const el = fakeElement({ left: 119.5, top: 40.6 })
    glideIntoPlace(el, ghost)
    assert.equal(el.animations.length, 0)
  })

  test('a pending landing runs once against the rendered element, then clears', () => {
    const el = fakeElement({ left: 0, top: 0 })
    const queried = []
    const root = { querySelector: (s) => (queried.push(s), el) }
    const pending = { current: { selector: '[data-island-id="f"]', rect: ghost } }
    runPendingGlide(root, pending)
    runPendingGlide(root, pending)
    assert.equal(pending.current, null)
    assert.deepEqual(queried, ['[data-island-id="f"]'])
    assert.equal(el.animations.length, 1)
  })

  test('a pending landing whose element is gone just clears', () => {
    const pending = { current: { selector: '#gone', rect: ghost } }
    runPendingGlide({ querySelector: () => null }, pending)
    assert.equal(pending.current, null)
    const alsoPending = { current: { selector: '#gone', rect: ghost } }
    runPendingGlide(null, alsoPending)
    assert.equal(alsoPending.current, null)
  })
})

describe('switching workspaces', () => {
  test('content slides in from the side the new workspace sits on', () => {
    const right = fakeElement()
    workspaceSlideIn(right, 'right')
    assert.deepEqual(right.animations[0].frames, [
      { transform: 'translateX(44px)', opacity: 0 },
      { transform: 'translateX(0)', opacity: 1 },
    ])
    assert.equal(right.animations[0].options.duration, 240)

    const left = fakeElement()
    workspaceSlideIn(left, 'left')
    assert.equal(left.animations[0].frames[0].transform, 'translateX(-44px)')
  })

  test('no slide without an element or with motion off', () => {
    workspaceSlideIn(null, 'right')
    bodyClasses.add('motion-off')
    const el = fakeElement()
    workspaceSlideIn(el, 'left')
    assert.equal(el.animations.length, 0)
  })
})

describe('optimistic cache patches', () => {
  const state = () => ({
    workspaces: [
      {
        id: 'w1',
        tab_order: ['a', 'b', 'c'],
        tab_folders: [
          { id: 'f1', position: 0 },
          { id: 'f2', position: 1 },
          { id: 'f3', position: 2 },
        ],
      },
      { id: 'w2', tab_order: ['x'], tab_folders: [{ id: 'g1', position: 0 }] },
    ],
    tabs: [
      { id: 'a', folder_id: null },
      { id: 'b', folder_id: 'f1' },
    ],
  })

  test('a tab reorder replaces only that workspace order, without mutating the cached state', () => {
    const before = state()
    const qc = fakeQueryClient(before)
    patchTabOrder(qc, 'w1', ['c', 'a', 'b'])
    assert.deepEqual(qc.keys, [['browser-state']])
    assert.deepEqual(qc.data.workspaces[0].tab_order, ['c', 'a', 'b'])
    assert.equal(qc.data.workspaces[1], before.workspaces[1])
    assert.deepEqual(before.workspaces[0].tab_order, ['a', 'b', 'c'])
  })

  test('a folder reorder renumbers the listed folders and leaves unlisted ones be', () => {
    const before = state()
    const qc = fakeQueryClient(before)
    patchFolderOrder(qc, 'w1', ['f3', 'f1'])
    assert.deepEqual(qc.keys, [['browser-state']])
    assert.deepEqual(qc.data.workspaces[0].tab_folders, [
      { id: 'f1', position: 1 },
      { id: 'f2', position: 1 },
      { id: 'f3', position: 0 },
    ])
    assert.equal(qc.data.workspaces[1], before.workspaces[1])
    assert.equal(before.workspaces[0].tab_folders[2].position, 2)
  })

  test('filing a tab changes only that tab folder', () => {
    const before = state()
    const qc = fakeQueryClient(before)
    patchTabFolder(qc, 'a', 'f2')
    patchTabFolder(qc, 'b', null)
    assert.deepEqual(qc.data.tabs, [
      { id: 'a', folder_id: 'f2' },
      { id: 'b', folder_id: null },
    ])
    assert.equal(before.tabs[0].folder_id, null)
  })

  test('an empty cache stays empty', () => {
    const qc = fakeQueryClient(undefined)
    patchTabOrder(qc, 'w1', [])
    patchFolderOrder(qc, 'w1', [])
    patchTabFolder(qc, 'a', null)
    assert.equal(qc.data, undefined)
    assert.equal(qc.keys.length, 3)
  })
})
