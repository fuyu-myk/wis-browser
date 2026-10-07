// The video popout script's in-page pop-out button: where it appears, when it hides, and what a click asks of native.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { loadPopout } from '../support/s3-popoutPage.mjs'

/** One video at (100, 50), 640x360: right edge 740, button spot (713, 230). */
function page(extra = []) {
  return loadPopout({ videos: [{ left: 100, top: 50, width: 640, height: 360 }, ...extra] })
}
const shown = (p) => p.hoverButton()?.style.display === 'block'
/** A detached element drawn over the page at the given box. */
const box = (p, left, top, width, height) =>
  Object.assign(p.document.createElement('div'), { rect: { left, top, width, height } })

describe('placing the button', () => {
  test('it appears beside the right edge of the video under the pointer, vertically centred', () => {
    const p = page()
    p.mouse(300, 200)
    const b = p.hoverButton()
    assert.equal(b.style.display, 'block')
    assert.equal(b.style.left, '696px')
    assert.equal(b.style.top, '213px')
    assert.equal(b.parentNode, p.root)
    assert.equal(b.title, 'Pop out video')
    assert.equal(b.type, 'button')
  })

  test('it is created once and reused', () => {
    const p = page()
    p.mouse(300, 200)
    p.mouse(5, 5)
    p.mouse(300, 200)
    assert.equal(p.root.children.filter((c) => c.id === '__wis-popout-hover').length, 1)
  })

  test('pointer moves within one frame are placed once, at the first position', () => {
    const p = page()
    p.move(300, 200)
    p.move(5, 5)
    assert.equal(p.hoverButton(), null, 'nothing until the frame')
    p.clock.frame()
    assert.ok(shown(p))
    p.move(5, 5)
    p.clock.frame()
    assert.ok(!shown(p), 'the next frame takes the next move')
  })

  test('it hides when the pointer leaves every video', () => {
    const p = page()
    p.mouse(300, 200)
    p.mouse(20, 20)
    assert.ok(!shown(p))
  })

  test('the largest video under the pointer gets it', () => {
    const p = page([{ left: 200, top: 100, width: 300, height: 200 }])
    p.mouse(300, 200)
    assert.equal(p.hoverButton().style.left, '696px')
  })

  test('a video that is too small or has no metadata gets none', () => {
    const small = loadPopout({ videos: [{ left: 0, top: 0, width: 170, height: 100 }] })
    small.mouse(50, 50)
    assert.ok(!shown(small))

    const bare = loadPopout({ videos: [{ left: 0, top: 0, videoWidth: 0 }] })
    bare.mouse(50, 50)
    assert.ok(!shown(bare))
  })

  test('scrolling hides it', () => {
    const p = page()
    p.mouse(300, 200)
    p.scroll()
    assert.ok(!shown(p))
    assert.deepEqual(p.docEvents.options('scroll'), [{ capture: true, passive: true }])
  })

  test('it darkens while hovered', () => {
    const p = page()
    p.mouse(300, 200)
    const b = p.hoverButton()
    b.fire('mouseenter')
    assert.equal(b.style.background, 'rgba(15,15,20,0.85)')
    b.fire('mouseleave')
    assert.equal(b.style.background, 'rgba(15,15,20,0.62)')
  })

  test('it never shows while a video is popped out', () => {
    const p = page()
    p.mouse(300, 200)
    p.enter()
    assert.ok(!shown(p), 'entering hides it')
    p.mouse(300, 200)
    assert.ok(!shown(p))
  })
})

describe('occlusion at the button spot', () => {
  test('a popup over the spot hides it', () => {
    const p = page()
    p.page.above.push(box(p, 650, 200, 200, 100))
    p.mouse(300, 200)
    assert.ok(!shown(p))
  })

  test('a cover at least 90% of the video, like a player overlay, keeps it', () => {
    const p = page()
    p.page.above.push(box(p, 164, 50, 576, 324))
    p.mouse(300, 200)
    assert.ok(shown(p))

    const q = page()
    q.page.above.push(box(q, 165, 50, 575, 360))
    q.mouse(300, 200)
    assert.ok(!shown(q))
  })

  test('the video, something inside it, or its own container on top keeps it', () => {
    const p = page()
    const [v] = p.videos
    p.page.above.push(v.parentNode)
    p.mouse(300, 200)
    assert.ok(shown(p), 'the container')

    const q = page()
    const track = q.videos[0].appendChild(q.document.createElement('track'))
    q.page.above.push(track)
    q.mouse(300, 200)
    assert.ok(shown(q), 'a child of the video')
  })

  test('the button itself, and what is inside it, are seen through', () => {
    const p = page()
    p.mouse(300, 200)
    const b = p.hoverButton()
    p.page.above.push(b, b.children[0])
    p.mouse(300, 200)
    assert.ok(shown(p))
  })

  test('an empty hit test hides it', () => {
    const p = page()
    p.document.elementsFromPoint = () => []
    p.mouse(300, 200)
    assert.ok(!shown(p))
  })
})

describe('clicking the button', () => {
  test('it asks native to pop out, hides itself, and keeps the click from the page', () => {
    const p = page()
    p.mouse(300, 200)
    const click = p.hoverButton().fire('click')
    assert.deepEqual(p.sent, [{ kind: 'request' }])
    assert.ok(!shown(p))
    assert.ok(click.defaultPrevented && click.propagationStopped)
  })
})
