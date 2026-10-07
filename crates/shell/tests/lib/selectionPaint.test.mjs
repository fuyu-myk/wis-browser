// Keeping a page selection visible while the page menu holds focus
// (`lib/selectionPaint.ts`), drawn through the CSS highlight API.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { beforeEach, describe, test } from 'node:test'

class FakeStyle {
  id = ''
  textContent = ''
}

class FakeHighlight {
  ranges = []
  add(range) {
    this.ranges.push(range)
  }
  get size() {
    return this.ranges.length
  }
}

let nodes
let appended
let frames
let registry

const fakeDocument = {
  getElementById: (id) => nodes.get(id) ?? null,
  createElement: () => new FakeStyle(),
  head: {
    appendChild(node) {
      appended.push(node)
      nodes.set(node.id, node)
    },
  },
}

globalThis.HTMLStyleElement = FakeStyle
globalThis.document = fakeDocument
globalThis.requestAnimationFrame = (cb) => frames.push(cb)

const { MAX_SELECTION_CHARS, clearSelectionPaint, paintSelection } = await import(
  '../../src/lib/selectionPaint.ts'
)

beforeEach(() => {
  nodes = new Map()
  appended = []
  frames = []
  registry = {
    entries: new Map(),
    set(name, highlight) {
      this.entries.set(name, highlight)
    },
    delete(name) {
      this.entries.delete(name)
    },
  }
  globalThis.CSS = { highlights: registry }
  globalThis.window = { Highlight: FakeHighlight }
  globalThis.document = fakeDocument
})

const flushFrames = () => frames.splice(0).forEach((cb) => cb())
const range = (label, collapsed = false) => ({ collapsed, cloneRange: () => ({ copyOf: label }) })
const selection = (...ranges) => ({ rangeCount: ranges.length, getRangeAt: (i) => ranges[i] })
const drawn = () => registry.entries.get('wis-menu-selection')
const styleText = () => nodes.get('wis-menu-selection-style')?.textContent

describe('painting', () => {
  test('copies of the non-empty ranges are drawn, and the native selection is hidden', () => {
    paintSelection(selection(range('a'), range('b', true), range('c')))
    assert.deepEqual(drawn().ranges, [{ copyOf: 'a' }, { copyOf: 'c' }])
    assert.equal(
      styleText(),
      '::selection{background-color:transparent}::highlight(wis-menu-selection){background-color:Highlight}',
    )
  })

  test('a collapsed selection draws nothing', () => {
    paintSelection(selection(range('a', true)))
    paintSelection(selection())
    assert.equal(drawn(), undefined)
    assert.equal(appended.length, 0)
  })

  test('painting again reuses the one style element', () => {
    paintSelection(selection(range('a')))
    paintSelection(selection(range('b')))
    assert.equal(appended.length, 1)
    assert.deepEqual(drawn().ranges, [{ copyOf: 'b' }])
  })

  test('without the highlight API nothing is drawn', () => {
    globalThis.CSS = {}
    paintSelection(selection(range('a')))
    globalThis.CSS = { highlights: registry }
    globalThis.window = {}
    paintSelection(selection(range('a')))
    assert.equal(drawn(), undefined)
    assert.equal(appended.length, 0)
  })

  test('a highlight API that throws leaves the page as it was', () => {
    globalThis.window = {
      Highlight: class {
        constructor() {
          throw new Error('unsupported')
        }
      },
    }
    assert.doesNotThrow(() => paintSelection(selection(range('a'))))
    assert.equal(appended.length, 0)
  })
})

describe('clearing', () => {
  test('the drawing is hidden at once and released on the next frame', () => {
    paintSelection(selection(range('a')))
    clearSelectionPaint()
    assert.equal(styleText(), '::highlight(wis-menu-selection){background-color:transparent}')
    assert.ok(drawn(), 'still registered until the frame')
    flushFrames()
    assert.equal(drawn(), undefined)
  })

  test('a paint before the frame keeps the newer drawing', () => {
    paintSelection(selection(range('old')))
    clearSelectionPaint()
    paintSelection(selection(range('new')))
    flushFrames()
    assert.deepEqual(drawn().ranges, [{ copyOf: 'new' }])
  })

  test('clearing with nothing painted adds no style element', () => {
    clearSelectionPaint()
    flushFrames()
    assert.equal(appended.length, 0)
  })

  test('without the highlight API clearing does nothing', () => {
    globalThis.CSS = {}
    clearSelectionPaint()
    assert.equal(frames.length, 0)
  })

  test('a document gone before the frame or before the clear is ignored', () => {
    paintSelection(selection(range('a')))
    clearSelectionPaint()
    registry.delete = () => {
      throw new Error('detached')
    }
    assert.doesNotThrow(flushFrames)

    globalThis.document = {
      getElementById: () => {
        throw new Error('detached')
      },
    }
    assert.doesNotThrow(clearSelectionPaint)
  })
})

test('the chrome carries out as much selection as the page script', () => {
  const page = readFileSync(new URL('../../src-tauri/src/page_menu.rs', import.meta.url), 'utf8')
  assert.equal(MAX_SELECTION_CHARS, Number(page.match(/var MAX_SELECTION = (\d+);/)[1]))
})
