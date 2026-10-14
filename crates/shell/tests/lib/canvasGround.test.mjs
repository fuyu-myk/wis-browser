// How far a canvas tab reaches past its pane into the chrome below it
// (`lib/canvasGround.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { canvasGround, readGround } from '../../src/lib/canvasGround.ts'

const pane = { x: 200, y: 60, width: 900, height: 728 }

describe('the canvas ground', () => {
  test('a canvas pane at the bottom reaches down to the window edge', () => {
    assert.equal(canvasGround({ rect: pane, hostBottom: 788, windowHeight: 800, canvas: true }), 12)
  })

  test('other tabs, upper panes and wide gaps keep their pane', () => {
    assert.equal(canvasGround({ rect: pane, hostBottom: 788, windowHeight: 800, canvas: false }), 0)
    assert.equal(canvasGround({ rect: pane, hostBottom: 1200, windowHeight: 1212, canvas: true }), 0, 'a pane sits below it')
    assert.equal(canvasGround({ rect: pane, hostBottom: 788, windowHeight: 900, canvas: true }), 0, 'something else lives below')
    assert.equal(canvasGround({ rect: pane, hostBottom: 788, windowHeight: 788, canvas: true }), 0, 'already at the edge')
  })

  test('sub-pixel layout still counts as the bottom, and the gap rounds to whole pixels', () => {
    assert.equal(canvasGround({ rect: { ...pane, height: 727.6 }, hostBottom: 788, windowHeight: 800, canvas: true }), 12)
  })
})

describe('the page reading its ground', () => {
  test('takes a pushed pair or a reply, and nothing odd', () => {
    assert.deepEqual(readGround('12,12'), { bottom: 12, radius: 12 })
    assert.deepEqual(readGround({ bottom: 12, radius: 6.6 }), { bottom: 12, radius: 6.6 })
    for (const junk of ['12', 'a,b', '-1,4', '12,12;alert(1)', null, { bottom: '1' }, { bottom: 500, radius: 1 }])
      assert.equal(readGround(junk), null, String(junk))
  })
})
