// The video popout's event names and its remembered position
// (`features/video-popout/videoPopout.ts`).

import assert from 'node:assert/strict'
import { beforeEach, describe, test } from 'node:test'

import * as popout from '../../src/features/video-popout/videoPopout.ts'
import { readSource } from '../support/l3-harness.mjs'

const KEY = 'wis-video-popout-geometry'

/** A Map-backed localStorage that can be made to throw. */
function fakeStorage() {
  const items = new Map()
  return {
    items,
    broken: false,
    getItem(key) {
      if (this.broken) throw new Error('storage unavailable')
      return items.has(key) ? items.get(key) : null
    },
    setItem(key, value) {
      if (this.broken) throw new Error('quota exceeded')
      items.set(key, String(value))
    },
  }
}

let storage
beforeEach(() => {
  storage = fakeStorage()
  Object.defineProperty(globalThis, 'localStorage', { value: storage, configurable: true })
})

describe('the remembered position', () => {
  test('is absent until one is saved', () => {
    assert.equal(popout.loadPopoutGeometry(), null)
  })

  test('round-trips through storage under its persisted key', () => {
    popout.savePopoutGeometry({ x: 1200, y: 80 })
    assert.deepEqual(JSON.parse(storage.items.get(KEY)), { x: 1200, y: 80 })
    assert.deepEqual(popout.loadPopoutGeometry(), { x: 1200, y: 80 })
  })

  test('accepts negative coordinates from a display left of or above the main one', () => {
    popout.savePopoutGeometry({ x: -1440, y: -200 })
    assert.deepEqual(popout.loadPopoutGeometry(), { x: -1440, y: -200 })
  })

  test('keeps only the position from a stored entry with extra fields', () => {
    storage.items.set(KEY, JSON.stringify({ x: 10, y: 20, width: 640, height: 360 }))
    assert.deepEqual(popout.loadPopoutGeometry(), { x: 10, y: 20 })
  })

  test('is discarded when the stored value is not a usable position', () => {
    for (const raw of ['', '{oops', 'null', '42', '"x"', '{"x":1}', '{"x":"1","y":2}', '{"x":null,"y":2}']) {
      storage.items.set(KEY, raw)
      assert.equal(popout.loadPopoutGeometry(), null, raw)
    }
  })

  test('is simply absent when storage cannot be read', () => {
    storage.broken = true
    assert.equal(popout.loadPopoutGeometry(), null)
  })

  test('a failed save is swallowed', () => {
    storage.broken = true
    assert.doesNotThrow(() => popout.savePopoutGeometry({ x: 1, y: 2 }))
  })
})

test('the event names are the ones video_popout emits', () => {
  const rust = readSource('src-tauri/src/video_popout/mod.rs')
  const expected = {
    MEDIA_EVENT: popout.POPOUT_MEDIA_EVENT,
    REQUESTED_EVENT: popout.POPOUT_REQUESTED_EVENT,
    OPENED_EVENT: popout.POPOUT_OPENED_EVENT,
    CLOSED_EVENT: popout.POPOUT_CLOSED_EVENT,
  }
  for (const [rustName, value] of Object.entries(expected)) {
    assert.ok(rust.includes(`const ${rustName}: &str = "${value}";`), `${rustName} = ${value}`)
  }
})
